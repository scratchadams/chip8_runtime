pub mod kernel {
    use std::collections::{HashMap, VecDeque};
    use std::fs;
    use std::io::{self, Error, ErrorKind, Read, Write};
    use std::path::{Component, Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use crate::display::display::{DisplayMode, DisplayWindow};
    use crate::proc::proc::{ConsoleMode, InputMode, Proc};
    use crate::shared_memory::shared_memory::SharedMemory;

    const SYS_SPAWN: u16 = 0x0101;
    const SYS_EXIT: u16 = 0x0102;
    const SYS_WAIT: u16 = 0x0103;
    const SYS_YIELD: u16 = 0x0104;
    const SYS_WRITE: u16 = 0x0110;
    const SYS_READ: u16 = 0x0111;
    const SYS_INPUT_MODE: u16 = 0x0112;
    const SYS_CONSOLE_MODE: u16 = 0x0113;
    const SYS_FS_LIST: u16 = 0x0120;
    const SYS_FS_OPEN: u16 = 0x0121;
    const SYS_FS_READ: u16 = 0x0122;
    const SYS_FS_CLOSE: u16 = 0x0123;

    const ERR_INVALID: u8 = 0x02;
    const ERR_IO: u8 = 0x03;
    const ERR_NOT_FOUND: u8 = 0x04;
    const ERR_NOT_DIR: u8 = 0x05;
    const ERR_IS_DIR: u8 = 0x06;
    const ERR_NAME_TOO_LONG: u8 = 0x07;
    const ERR_TOO_MANY_OPEN: u8 = 0x08;
    const ERR_PATH: u8 = 0x09;

    const MAX_FILENAME_LEN: usize = 64;
    const MAX_DIR_ENTRIES: usize = 256;
    const MAX_FILE_SIZE: u64 = 64 * 1024;
    const MAX_OPEN_FILES: usize = 32;
    const DIR_ENTRY_SIZE: usize = 1 + MAX_FILENAME_LEN + 1 + 4;

    // syscall handlers return scheduling outcome for the caller.
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum SyscallOutcome {
        Completed,
        Blocked,
        Yielded,
    }

    pub type SyscallHandler =
        Arc<dyn Fn(&mut Kernel, u32, &mut Proc) -> SyscallOutcome + Send + Sync>;

    pub struct SyscallTable {
        handlers: HashMap<u16, SyscallHandler>,
    }

    impl SyscallTable {
        /// create an empty syscall table with no registered IDs.
        /// Example: `let table = SyscallTable::new();`
        pub fn new() -> SyscallTable {
            SyscallTable {
                handlers: HashMap::new(),
            }
        }

        /// register a syscall handler in the reserved ID range (0x0100..0x01FF).
        pub fn register<H>(&mut self, id: u16, handler: H) -> Result<(), Error>
        where
            H: Fn(&mut Kernel, u32, &mut Proc) -> SyscallOutcome + Send + Sync + 'static,
        {
            if !(0x0100..0x0200).contains(&id) {
                return Err(Error::new(ErrorKind::InvalidInput, "syscall id out of range"));
            }
            self.handlers.insert(id, Arc::new(handler));
            Ok(())
        }

        /// look up a handler by syscall ID without executing it.
        pub fn handler(&self, id: u16) -> Option<SyscallHandler> {
            self.handlers.get(&id).cloned()
        }
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum ProcState {
        Running,
        Blocked,
        Exited,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    enum WaitTarget {
        Pid(u32),
        Read { buf: u16, len: u16, mode: InputMode },
    }

    struct ProcEntry {
        proc: Proc,
        state: ProcState,
        exit_code: Option<u8>,
        waiting_for: Option<WaitTarget>,
    }

    pub struct Kernel {
        mem: Arc<Mutex<SharedMemory>>,
        syscalls: SyscallTable,
        procs: HashMap<u32, ProcEntry>,
        next_pid: u32,
        root_dir: PathBuf,
        input: VecDeque<u8>,
        pending_exit: HashMap<u32, u8>,
        pending_block: HashMap<u32, WaitTarget>,
    }

    impl Kernel {
        /// build a kernel with shared memory and an empty syscall registry.
        pub fn new(mem: Arc<Mutex<SharedMemory>>, root_dir: PathBuf) -> Result<Kernel, Error> {
            let root = root_dir
                .canonicalize()
                .map_err(|e| Error::new(ErrorKind::InvalidInput, format!("invalid root dir: {e}")))?;
            Self::validate_root_layout(&root)?;
            Ok(Kernel {
                mem,
                syscalls: SyscallTable::new(),
                procs: HashMap::new(),
                next_pid: 1,
                root_dir: root,
                input: VecDeque::new(),
                pending_exit: HashMap::new(),
                pending_block: HashMap::new(),
            })
        }

        /// register base syscalls using the shared registry.
        pub fn register_base_syscalls(&mut self) -> Result<(), Error> {
            self.register_syscall(SYS_SPAWN, sys_spawn)?;
            self.register_syscall(SYS_EXIT, sys_exit)?;
            self.register_syscall(SYS_WAIT, sys_wait)?;
            self.register_syscall(SYS_YIELD, sys_yield)?;
            self.register_syscall(SYS_WRITE, sys_write)?;
            self.register_syscall(SYS_READ, sys_read)?;
            self.register_syscall(SYS_INPUT_MODE, sys_input_mode)?;
            self.register_syscall(SYS_CONSOLE_MODE, sys_console_mode)?;
            self.register_syscall(SYS_FS_LIST, sys_fs_list)?;
            self.register_syscall(SYS_FS_OPEN, sys_fs_open)?;
            self.register_syscall(SYS_FS_READ, sys_fs_read)?;
            self.register_syscall(SYS_FS_CLOSE, sys_fs_close)?;
            Ok(())
        }

        /// register a syscall handler on the shared registry.
        pub fn register_syscall<H>(&mut self, id: u16, handler: H) -> Result<(), Error>
        where
            H: Fn(&mut Kernel, u32, &mut Proc) -> SyscallOutcome + Send + Sync + 'static,
        {
            self.syscalls.register(id, handler)
        }

        /// create a new Proc bound to this kernel's shared memory.
        pub fn spawn_proc(&mut self, display: DisplayWindow, pages: u16) -> Result<u32, Error> {
            let pid = self.next_pid;
            self.next_pid = self.next_pid.wrapping_add(1);

            let proc = Proc::new_with_display_and_pages(
                Arc::clone(&self.mem),
                display,
                pages,
            )?;

            self.procs.insert(
                pid,
                ProcEntry {
                    proc,
                    state: ProcState::Running,
                    exit_code: None,
                    waiting_for: None,
                },
            );
            Ok(pid)
        }

        /// spawn and load a ROM in one step.
        pub fn spawn_proc_with_rom(
            &mut self,
            display: DisplayWindow,
            pages: u16,
            rom_path: &Path,
        ) -> Result<u32, Error> {
            let pid = self.spawn_proc(display, pages)?;
            self.load_rom(pid, rom_path)?;
            Ok(pid)
        }

        /// spawn a ROM by name, resolved relative to the kernel root.
        pub fn spawn_proc_from_name(
            &mut self,
            display: DisplayWindow,
            pages: u16,
            name: &str,
        ) -> Result<u32, Error> {
            let path = self.resolve_rom_path(name)?;
            self.spawn_proc_with_rom(display, pages, &path)
        }

        #[allow(dead_code)]
        /// access a process by pid for inspection (tests/tools).
        pub fn proc(&self, pid: u32) -> Option<&Proc> {
            self.procs.get(&pid).map(|entry| &entry.proc)
        }

        #[allow(dead_code)]
        /// access a process by pid for mutation (tests/debug only).
        pub fn proc_mut(&mut self, pid: u32) -> Option<&mut Proc> {
            self.procs.get_mut(&pid).map(|entry| &mut entry.proc)
        }

        #[allow(dead_code)]
        /// read the current scheduler state for a pid.
        pub fn proc_state(&self, pid: u32) -> Option<ProcState> {
            self.procs.get(&pid).map(|entry| entry.state)
        }

        #[allow(dead_code)]
        /// step a single pid once for tests or manual scheduling.
        pub fn step_proc(&mut self, pid: u32) -> Result<SyscallOutcome, Error> {
            let mut entry = self
                .procs
                .remove(&pid)
                .ok_or_else(|| Error::new(ErrorKind::NotFound, "pid not found"))?;

            if entry.state != ProcState::Running {
                self.procs.insert(pid, entry);
                return Ok(SyscallOutcome::Completed);
            }

            let outcome = entry
                .proc
                .step(|id, proc| self.dispatch_syscall(pid, proc, id));

            self.apply_pending(pid, &mut entry, outcome);
            self.procs.insert(pid, entry);
            Ok(outcome)
        }

        /// load a ROM into an existing process by pid.
        pub fn load_rom(&mut self, pid: u32, rom_path: &Path) -> Result<(), Error> {
            let entry = self
                .procs
                .get_mut(&pid)
                .ok_or_else(|| Error::new(ErrorKind::NotFound, "pid not found"))?;
            entry.proc.load_program(rom_path.to_string_lossy().to_string())
        }

        /// run the cooperative scheduler until no runnable procs remain.
        pub fn run(&mut self) -> Result<(), Error> {
            loop {
                self.poll_console_input();
                let mut ran_any = false;
                let pids: Vec<u32> = self.procs.keys().copied().collect();
                for pid in pids {
                    if !self.is_runnable(pid) {
                        continue;
                    }
                    ran_any = true;
                    self.run_proc_until_yield_or_block(pid)?;
                }

                if ran_any {
                    continue;
                }

                if self.any_blocked_on_read_line_host() {
                    self.blocking_read_line_from_stdin()?;
                    continue;
                }

                if self.any_blocked_on_read_byte_host() {
                    self.blocking_read_byte_from_stdin()?;
                    continue;
                }

                if self.any_blocked() {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }

                break;
            }
            Ok(())
        }

        /// inject host input into the kernel and wake blocked readers.
        pub fn push_input(&mut self, data: &[u8]) {
            self.input.extend(data);
            self.unblock_readers();
        }

        /// inject console input for a specific pid (tests/tools).
        #[allow(dead_code)]
        pub fn push_console_input(&mut self, pid: u32, data: &[u8]) {
            if let Some(entry) = self.procs.get_mut(&pid) {
                entry.proc.console_input.extend(data);
            }
            self.unblock_readers();
        }

        fn poll_console_input(&mut self) {
            let pids: Vec<u32> = self.procs.keys().copied().collect();
            let mut saw_input = false;

            for pid in pids {
                let mut entry = match self.procs.remove(&pid) {
                    Some(entry) => entry,
                    None => continue,
                };

                if entry.proc.console_mode == ConsoleMode::Display && entry.state == ProcState::Blocked {
                    entry.proc.display.poll_input(true);
                    let data = entry.proc.display.drain_text_input();
                    if !data.is_empty() {
                        self.apply_console_input(&mut entry.proc, &data);
                        saw_input = true;
                    }
                }

                self.procs.insert(pid, entry);
            }

            if saw_input {
                self.unblock_readers();
            }
        }

        fn apply_console_input(&mut self, proc: &mut Proc, data: &[u8]) {
            for &byte in data {
                if byte == 0x08 {
                    if proc.console_input.pop_back().is_some() {
                        proc.display.console_backspace();
                    }
                    continue;
                }
                proc.console_input.push_back(byte);
                proc.display.console_write(&[byte]);
            }
        }

        fn run_proc_until_yield_or_block(&mut self, pid: u32) -> Result<(), Error> {
            loop {
                let mut entry = self
                    .procs
                    .remove(&pid)
                    .ok_or_else(|| Error::new(ErrorKind::NotFound, "pid not found"))?;
                if entry.state != ProcState::Running {
                    self.procs.insert(pid, entry);
                    return Ok(());
                }

                let outcome = entry
                    .proc
                    .step(|id, proc| self.dispatch_syscall(pid, proc, id));

                self.apply_pending(pid, &mut entry, outcome);
                let should_break = matches!(outcome, SyscallOutcome::Blocked | SyscallOutcome::Yielded);
                self.procs.insert(pid, entry);
                if should_break {
                    break;
                }
            }
            Ok(())
        }

        fn dispatch_syscall(&mut self, pid: u32, proc: &mut Proc, id: u16) -> Result<SyscallOutcome, Error> {
            let handler = self
                .syscalls
                .handler(id)
                .ok_or_else(|| Error::new(ErrorKind::NotFound, "unknown syscall id"))?;
            Ok(handler(self, pid, proc))
        }

        fn apply_pending(&mut self, pid: u32, entry: &mut ProcEntry, outcome: SyscallOutcome) {
            if let Some(code) = self.pending_exit.remove(&pid) {
                entry.state = ProcState::Exited;
                entry.exit_code = Some(code);
                entry.waiting_for = None;
                self.unblock_waiters(pid, code);
            } else if let Some(wait) = self.pending_block.remove(&pid) {
                entry.state = ProcState::Blocked;
                entry.waiting_for = Some(wait);
            } else if outcome == SyscallOutcome::Blocked {
                entry.state = ProcState::Blocked;
            }
        }

        fn is_runnable(&self, pid: u32) -> bool {
            self.procs
                .get(&pid)
                .map(|entry| entry.state == ProcState::Running)
                .unwrap_or(false)
        }

        fn any_blocked(&self) -> bool {
            self.procs
                .values()
                .any(|entry| entry.state == ProcState::Blocked)
        }

        fn any_blocked_on_read_line_host(&self) -> bool {
            self.procs.values().any(|entry| match entry.waiting_for {
                Some(WaitTarget::Read { mode: InputMode::Line, .. }) => {
                    entry.proc.console_mode == ConsoleMode::Host
                }
                _ => false,
            })
        }

        fn any_blocked_on_read_byte_host(&self) -> bool {
            self.procs.values().any(|entry| match entry.waiting_for {
                Some(WaitTarget::Read { mode: InputMode::Byte, .. }) => {
                    entry.proc.console_mode == ConsoleMode::Host
                }
                _ => false,
            })
        }

        fn unblock_waiters(&mut self, waited_pid: u32, code: u8) {
            for entry in self.procs.values_mut() {
                if entry.state != ProcState::Blocked {
                    continue;
                }
                if let Some(WaitTarget::Pid(pid)) = entry.waiting_for {
                    if pid == waited_pid {
                        entry.proc.regs.V[0] = code;
                        entry.proc.regs.V[0xF] = 0;
                        entry.state = ProcState::Running;
                        entry.waiting_for = None;
                    }
                }
            }
        }

        fn unblock_readers(&mut self) {
            // console-backed readers: each proc has its own input queue.
            for entry in self.procs.values_mut() {
                if entry.state != ProcState::Blocked {
                    continue;
                }
                if entry.proc.console_mode != ConsoleMode::Display {
                    continue;
                }
                let Some(WaitTarget::Read { buf, len, mode: InputMode::Line }) = entry.waiting_for else {
                    continue;
                };
                let Some(newline_idx) = Self::find_newline_in(&entry.proc.console_input) else {
                    continue;
                };
                let count = (len as usize).min(newline_idx + 1);
                let data = Self::pop_input(&mut entry.proc.console_input, count);
                if entry.proc.write_bytes(buf as u32, &data).is_err() {
                    entry.proc.regs.V[0] = ERR_INVALID;
                    entry.proc.regs.V[0xF] = 1;
                } else {
                    entry.proc.regs.V[0] = count.min(0xFF) as u8;
                    entry.proc.regs.V[0xF] = 0;
                }
                entry.state = ProcState::Running;
                entry.waiting_for = None;
            }

            for entry in self.procs.values_mut() {
                if entry.state != ProcState::Blocked {
                    continue;
                }
                if entry.proc.console_mode != ConsoleMode::Display {
                    continue;
                }
                let Some(WaitTarget::Read { buf, len, mode: InputMode::Byte }) = entry.waiting_for else {
                    continue;
                };
                if entry.proc.console_input.is_empty() {
                    continue;
                }
                let count = (len as usize).min(entry.proc.console_input.len());
                let data = Self::pop_input(&mut entry.proc.console_input, count);
                if entry.proc.write_bytes(buf as u32, &data).is_err() {
                    entry.proc.regs.V[0] = ERR_INVALID;
                    entry.proc.regs.V[0xF] = 1;
                } else {
                    entry.proc.regs.V[0] = count.min(0xFF) as u8;
                    entry.proc.regs.V[0xF] = 0;
                }
                entry.state = ProcState::Running;
                entry.waiting_for = None;
            }

            if self.input.is_empty() {
                return;
            }

            let (procs, input) = (&mut self.procs, &mut self.input);

            // host-backed readers: line mode first, then byte mode.
            for entry in procs.values_mut() {
                if entry.state != ProcState::Blocked {
                    continue;
                }
                if entry.proc.console_mode != ConsoleMode::Host {
                    continue;
                }
                let Some(WaitTarget::Read { buf, len, mode: InputMode::Line }) = entry.waiting_for else {
                    continue;
                };
                let Some(newline_idx) = Self::find_newline_in(input) else {
                    continue;
                };
                let count = (len as usize).min(newline_idx + 1);
                let data = Self::pop_input(input, count);
                if entry.proc.write_bytes(buf as u32, &data).is_err() {
                    entry.proc.regs.V[0] = ERR_INVALID;
                    entry.proc.regs.V[0xF] = 1;
                } else {
                    entry.proc.regs.V[0] = count.min(0xFF) as u8;
                    entry.proc.regs.V[0xF] = 0;
                }
                entry.state = ProcState::Running;
                entry.waiting_for = None;
            }

            for entry in procs.values_mut() {
                if entry.state != ProcState::Blocked {
                    continue;
                }
                if entry.proc.console_mode != ConsoleMode::Host {
                    continue;
                }
                let Some(WaitTarget::Read { buf, len, mode: InputMode::Byte }) = entry.waiting_for else {
                    continue;
                };
                if input.is_empty() {
                    break;
                }
                let count = (len as usize).min(input.len());
                let data = Self::pop_input(input, count);
                if entry.proc.write_bytes(buf as u32, &data).is_err() {
                    entry.proc.regs.V[0] = ERR_INVALID;
                    entry.proc.regs.V[0xF] = 1;
                } else {
                    entry.proc.regs.V[0] = count.min(0xFF) as u8;
                    entry.proc.regs.V[0xF] = 0;
                }
                entry.state = ProcState::Running;
                entry.waiting_for = None;
            }
        }

        fn blocking_read_line_from_stdin(&mut self) -> Result<(), Error> {
            let mut buf = String::new();
            let bytes = io::stdin().read_line(&mut buf)?;
            if bytes == 0 {
                return Ok(());
            }
            self.push_input(buf.as_bytes());
            Ok(())
        }

        fn blocking_read_byte_from_stdin(&mut self) -> Result<(), Error> {
            let mut buf = [0u8; 1];
            let bytes = io::stdin().read(&mut buf)?;
            if bytes == 0 {
                return Ok(());
            }
            self.push_input(&buf[..bytes]);
            Ok(())
        }

        fn find_newline_in(input: &VecDeque<u8>) -> Option<usize> {
            input.iter().position(|&b| b == b'\n')
        }

        fn pop_input(input: &mut VecDeque<u8>, count: usize) -> Vec<u8> {
            let mut data = Vec::with_capacity(count);
            for _ in 0..count {
                if let Some(byte) = input.pop_front() {
                    data.push(byte);
                } else {
                    break;
                }
            }
            data
        }

        fn resolve_rom_path(&self, name: &str) -> Result<PathBuf, Error> {
            let candidate = self.root_dir.join(name);
            let canon = candidate
                .canonicalize()
                .map_err(|e| Error::new(ErrorKind::NotFound, format!("rom not found: {e}")))?;
            if !canon.starts_with(&self.root_dir) {
                return Err(Error::new(ErrorKind::PermissionDenied, "rom path escapes root"));
            }
            Ok(canon)
        }

        fn resolve_fs_path(&self, name: &str) -> Result<PathBuf, Error> {
            if name.is_empty() || name == "." {
                return Ok(self.root_dir.clone());
            }

            let rel = Path::new(name);
            if rel.is_absolute() {
                return Err(Error::new(ErrorKind::InvalidInput, "absolute paths not allowed"));
            }

            for comp in rel.components() {
                match comp {
                    Component::CurDir => {}
                    Component::ParentDir => {
                        return Err(Error::new(ErrorKind::InvalidInput, "parent dir not allowed"));
                    }
                    Component::Normal(seg) => {
                        let seg_len = seg.to_string_lossy().len();
                        if seg_len > MAX_FILENAME_LEN {
                            return Err(Error::new(
                                ErrorKind::InvalidInput,
                                format!("path segment exceeds {MAX_FILENAME_LEN} bytes: {seg_len}"),
                            ));
                        }
                    }
                    _ => {
                        return Err(Error::new(ErrorKind::InvalidInput, "invalid path component"));
                    }
                }
            }

            let candidate = self.root_dir.join(rel);
            let canon = candidate
                .canonicalize()
                .map_err(|e| Error::new(ErrorKind::NotFound, format!("path not found: {e}")))?;
            if !canon.starts_with(&self.root_dir) {
                return Err(Error::new(ErrorKind::PermissionDenied, "path escapes root"));
            }
            Ok(canon)
        }

        fn validate_root_layout(root: &Path) -> Result<(), Error> {
            let mut stack = vec![root.to_path_buf()];
            while let Some(dir) = stack.pop() {
                let mut count = 0usize;
                for entry in fs::read_dir(&dir)
                    .map_err(|e| Error::new(ErrorKind::InvalidInput, format!("cannot read {dir:?}: {e}")))? {
                    let entry = entry.map_err(|e| {
                        Error::new(ErrorKind::InvalidInput, format!("cannot read dir entry in {dir:?}: {e}"))
                    })?;
                    count += 1;
                    if count > MAX_DIR_ENTRIES {
                        return Err(Error::new(
                            ErrorKind::InvalidInput,
                            format!(
                                "directory {:?} exceeds max entries ({MAX_DIR_ENTRIES})",
                                dir
                            ),
                        ));
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.len() > MAX_FILENAME_LEN {
                        return Err(Error::new(
                            ErrorKind::InvalidInput,
                            format!(
                                "entry name too long in {:?}: '{}' (max {MAX_FILENAME_LEN})",
                                dir, name
                            ),
                        ));
                    }
                    let meta = fs::symlink_metadata(entry.path())
                        .map_err(|e| Error::new(ErrorKind::InvalidInput, format!("metadata error for {:?}: {e}", entry.path())))?;
                    if meta.file_type().is_symlink() {
                        return Err(Error::new(
                            ErrorKind::InvalidInput,
                            format!("symlink not allowed in root: {:?}", entry.path()),
                        ));
                    }
                    if meta.is_dir() {
                        stack.push(entry.path());
                    } else if meta.is_file() && meta.len() > MAX_FILE_SIZE {
                        return Err(Error::new(
                            ErrorKind::InvalidInput,
                            format!(
                                "file too large: {:?} ({} bytes, max {MAX_FILE_SIZE})",
                                entry.path(),
                                meta.len()
                            ),
                        ));
                    }
                }
            }
            Ok(())
        }

        fn syscall_arg(proc: &mut Proc, index: usize) -> Result<u16, Error> {
            let base = proc.regs.I as u32;
            let frame_len = proc.read_u8(base)? as usize;
            let offset = 1 + index * 2;
            if offset + 1 >= frame_len {
                return Err(Error::new(ErrorKind::InvalidInput, "syscall frame too small"));
            }
            proc.read_u16(base + offset as u32)
        }
    }

    fn sys_spawn(kernel: &mut Kernel, _pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let name_ptr = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let name_len = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let pages = Kernel::syscall_arg(proc, 2).unwrap_or(1);
        let name_bytes = match proc.read_bytes(name_ptr as u32, name_len as usize) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let rom_name = String::from_utf8_lossy(&name_bytes).to_string();
        let path = match kernel.resolve_rom_path(&rom_name) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_IO;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let display = match DisplayWindow::from_env() {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_IO;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        match kernel.spawn_proc_with_rom(display, pages, &path) {
            Ok(pid) => {
                proc.regs.V[0] = (pid & 0xFF) as u8;
                proc.regs.V[0xF] = 0;
            }
            Err(_) => {
                proc.regs.V[0] = ERR_IO;
                proc.regs.V[0xF] = 1;
            }
        }
        SyscallOutcome::Completed
    }

    fn sys_exit(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let code = Kernel::syscall_arg(proc, 0).unwrap_or(0) as u8;
        kernel.pending_exit.insert(pid, code);
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_wait(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let target = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val as u32,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let Some(target_entry) = kernel.procs.get(&target) else {
            proc.regs.V[0] = ERR_INVALID;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        };

        if target_entry.state == ProcState::Exited {
            proc.regs.V[0] = target_entry.exit_code.unwrap_or(0);
            proc.regs.V[0xF] = 0;
            return SyscallOutcome::Completed;
        }

        kernel
            .pending_block
            .insert(pid, WaitTarget::Pid(target));
        SyscallOutcome::Blocked
    }

    fn sys_yield(_kernel: &mut Kernel, _pid: u32, proc: &mut Proc) -> SyscallOutcome {
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Yielded
    }

    fn sys_write(_kernel: &mut Kernel, _pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let buf = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let len = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let data = match proc.read_bytes(buf as u32, len as usize) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        if proc.console_mode == ConsoleMode::Display {
            proc.display.console_write(&data);
            proc.regs.V[0] = (data.len().min(0xFF)) as u8;
            proc.regs.V[0xF] = 0;
            return SyscallOutcome::Completed;
        }

        let mut stdout = io::stdout();
        if stdout.write_all(&data).is_err() {
            proc.regs.V[0] = ERR_IO;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }
        let _ = stdout.flush();
        proc.regs.V[0] = (data.len().min(0xFF)) as u8;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_read(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let buf = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let len = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let mode = proc.input_mode;

        match mode {
            InputMode::Line => {
                if proc.console_mode == ConsoleMode::Display {
                    let Some(newline_idx) = Kernel::find_newline_in(&proc.console_input) else {
                        kernel
                            .pending_block
                            .insert(pid, WaitTarget::Read { buf, len, mode });
                        return SyscallOutcome::Blocked;
                    };
                    let count = (len as usize).min(newline_idx + 1);
                    let data = Kernel::pop_input(&mut proc.console_input, count);
                    if proc.write_bytes(buf as u32, &data).is_err() {
                        proc.regs.V[0] = ERR_INVALID;
                        proc.regs.V[0xF] = 1;
                    } else {
                        proc.regs.V[0] = count.min(0xFF) as u8;
                        proc.regs.V[0xF] = 0;
                    }
                    return SyscallOutcome::Completed;
                }

                let Some(newline_idx) = Kernel::find_newline_in(&kernel.input) else {
                    kernel
                        .pending_block
                        .insert(pid, WaitTarget::Read { buf, len, mode });
                    return SyscallOutcome::Blocked;
                };
                let count = (len as usize).min(newline_idx + 1);
                let data = Kernel::pop_input(&mut kernel.input, count);
                if proc.write_bytes(buf as u32, &data).is_err() {
                    proc.regs.V[0] = ERR_INVALID;
                    proc.regs.V[0xF] = 1;
                } else {
                    proc.regs.V[0] = count.min(0xFF) as u8;
                    proc.regs.V[0xF] = 0;
                }
                SyscallOutcome::Completed
            }
            InputMode::Byte => {
                if proc.console_mode == ConsoleMode::Display {
                    if proc.console_input.is_empty() {
                        kernel
                            .pending_block
                            .insert(pid, WaitTarget::Read { buf, len, mode });
                        return SyscallOutcome::Blocked;
                    }
                    let count = (len as usize).min(proc.console_input.len());
                    let data = Kernel::pop_input(&mut proc.console_input, count);
                    if proc.write_bytes(buf as u32, &data).is_err() {
                        proc.regs.V[0] = ERR_INVALID;
                        proc.regs.V[0xF] = 1;
                    } else {
                        proc.regs.V[0] = count.min(0xFF) as u8;
                        proc.regs.V[0xF] = 0;
                    }
                    return SyscallOutcome::Completed;
                }

                if kernel.input.is_empty() {
                    kernel
                        .pending_block
                        .insert(pid, WaitTarget::Read { buf, len, mode });
                    return SyscallOutcome::Blocked;
                }
                let count = (len as usize).min(kernel.input.len());
                let data = Kernel::pop_input(&mut kernel.input, count);
                if proc.write_bytes(buf as u32, &data).is_err() {
                    proc.regs.V[0] = ERR_INVALID;
                    proc.regs.V[0xF] = 1;
                } else {
                    proc.regs.V[0] = count.min(0xFF) as u8;
                    proc.regs.V[0xF] = 0;
                }
                SyscallOutcome::Completed
            }
        }
    }

    fn sys_input_mode(_kernel: &mut Kernel, _pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let mode = match Kernel::syscall_arg(proc, 0) {
            Ok(0) => InputMode::Line,
            Ok(1) => InputMode::Byte,
            Ok(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        proc.input_mode = mode;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_console_mode(_kernel: &mut Kernel, _pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let mode = match Kernel::syscall_arg(proc, 0) {
            Ok(0) => ConsoleMode::Host,
            Ok(1) => ConsoleMode::Display,
            Ok(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        proc.console_mode = mode;
        proc.console_input.clear();
        match mode {
            ConsoleMode::Display => {
                proc.display.set_mode(DisplayMode::Console);
            }
            ConsoleMode::Host => {
                proc.display.set_mode(DisplayMode::Chip8);
            }
        }
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_fs_list(kernel: &mut Kernel, _pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let path_ptr = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let path_len = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let out_ptr = match Kernel::syscall_arg(proc, 2) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let max_entries = match Kernel::syscall_arg(proc, 3) {
            Ok(val) => val as usize,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let path_bytes = match proc.read_bytes(path_ptr as u32, path_len as usize) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let path_str = String::from_utf8_lossy(&path_bytes).to_string();
        let dir_path = match kernel.resolve_fs_path(&path_str) {
            Ok(val) => val,
            Err(err) => {
                proc.regs.V[0] = if err.kind() == ErrorKind::NotFound {
                    ERR_NOT_FOUND
                } else {
                    ERR_PATH
                };
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let meta = match fs::metadata(&dir_path) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_NOT_FOUND;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        if !meta.is_dir() {
            proc.regs.V[0] = ERR_NOT_DIR;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }

        let mut count = 0usize;
        let entries = match fs::read_dir(&dir_path) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_IO;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        for entry in entries {
            if count >= max_entries {
                break;
            }
            let entry = match entry {
                Ok(val) => val,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if name.len() > MAX_FILENAME_LEN {
                proc.regs.V[0] = ERR_NAME_TOO_LONG;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
            let meta = match entry.metadata() {
                Ok(val) => val,
                Err(_) => continue,
            };
            let kind = if meta.is_dir() { 1u8 } else { 0u8 };
            let size = if meta.is_file() { meta.len() as u32 } else { 0u32 };

            let mut record = Vec::with_capacity(DIR_ENTRY_SIZE);
            record.push(name.len() as u8);
            record.extend_from_slice(name.as_bytes());
            if name.len() < MAX_FILENAME_LEN {
                record.extend(std::iter::repeat(0u8).take(MAX_FILENAME_LEN - name.len()));
            }
            record.push(kind);
            record.extend_from_slice(&size.to_be_bytes());

            let addr = out_ptr as u32 + (count * DIR_ENTRY_SIZE) as u32;
            if proc.write_bytes(addr, &record).is_err() {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
            count += 1;
        }

        proc.regs.V[0] = count.min(0xFF) as u8;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_fs_open(kernel: &mut Kernel, _pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let path_ptr = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let path_len = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let _flags = Kernel::syscall_arg(proc, 2).unwrap_or(0);

        let path_bytes = match proc.read_bytes(path_ptr as u32, path_len as usize) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let path_str = String::from_utf8_lossy(&path_bytes).to_string();
        let file_path = match kernel.resolve_fs_path(&path_str) {
            Ok(val) => val,
            Err(err) => {
                proc.regs.V[0] = if err.kind() == ErrorKind::NotFound {
                    ERR_NOT_FOUND
                } else {
                    ERR_PATH
                };
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let meta = match fs::metadata(&file_path) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_NOT_FOUND;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        if meta.is_dir() {
            proc.regs.V[0] = ERR_IS_DIR;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }
        if meta.len() > MAX_FILE_SIZE {
            proc.regs.V[0] = ERR_IO;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }

        if proc.fds.len() >= MAX_OPEN_FILES {
            proc.regs.V[0] = ERR_TOO_MANY_OPEN;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }

        let file = match fs::File::open(&file_path) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_IO;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let mut fd = proc.next_fd;
        for _ in 0..=u8::MAX {
            if fd == 0 {
                fd = 1;
            }
            if !proc.fds.contains_key(&fd) {
                break;
            }
            fd = fd.wrapping_add(1);
        }
        if proc.fds.contains_key(&fd) {
            proc.regs.V[0] = ERR_TOO_MANY_OPEN;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }

        proc.fds.insert(fd, file);
        proc.next_fd = fd.wrapping_add(1);

        proc.regs.V[0] = fd;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_fs_read(_kernel: &mut Kernel, _pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let fd = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val as u8,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let buf = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let len = match Kernel::syscall_arg(proc, 2) {
            Ok(val) => val as usize,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let file = match proc.fds.get_mut(&fd) {
            Some(val) => val,
            None => {
                proc.regs.V[0] = ERR_NOT_FOUND;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let max_len = len.min(0xFF);
        let mut data = vec![0u8; max_len];
        let read = match file.read(&mut data) {
            Ok(val) => val,
            Err(_) => {
                proc.regs.V[0] = ERR_IO;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        data.truncate(read);
        if proc.write_bytes(buf as u32, &data).is_err() {
            proc.regs.V[0] = ERR_INVALID;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }

        proc.regs.V[0] = read.min(0xFF) as u8;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_fs_close(_kernel: &mut Kernel, _pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let fd = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val as u8,
            Err(_) => {
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        if proc.fds.remove(&fd).is_none() {
            proc.regs.V[0] = ERR_NOT_FOUND;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }
}
