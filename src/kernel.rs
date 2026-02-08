pub mod kernel {
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::fs;
    use std::io::{self, Error, ErrorKind, Read, Write};
    use std::path::{Component, Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::display::display::{DisplayMode, DisplayWindow};
    use crate::proc::proc::{ConsoleMode, Context, InputMode, Proc};
    use crate::shared_memory::shared_memory::SharedMemory;

    pub use chip8_core::syscall::syscall::SyscallOutcome;
    use chip8_core::device::device::{
        DisplayDevice, FsDevice, FsEntry, FsEntryKind, FsError, InputDevice, InputError,
    };

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
    const SYS_FS_WRITE: u16 = 0x0124;
    const SYS_DBG_LIST: u16 = 0x0130;
    const SYS_DBG_REGS: u16 = 0x0131;
    const SYS_DBG_MEM_READ: u16 = 0x0132;
    const SYS_DBG_MEM_WRITE: u16 = 0x0133;
    const SYS_DBG_TRACE_READ: u16 = 0x0134;
    const SYS_PERF_MEM_STATS: u16 = 0x0140;
    const SYS_PERF_PROC_INFO: u16 = 0x0141;
    const SYS_SET_TIMING: u16 = 0x0142;

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
    const DBG_PROC_RECORD_SIZE: usize = 8;
    const DBG_REGS_SIZE: usize = 24;
    const DEFAULT_TIMESLICE_STEPS: u32 = 200;
    const DEBUG_INPUT_ENV: &str = "CHIP8_DEBUG_INPUT";
    const SYSCALL_ERROR_LOG_ENV: &str = "CHIP8_SYSCALL_ERRORS";
    const TRACE_RECORD_SIZE: usize = 8;
    const TRACE_DEFAULT_CAPACITY: usize = 1024;

    const TRACE_KIND_SCHED: u8 = 0x01;
    const TRACE_KIND_SYSCALL: u8 = 0x02;

    const TRACE_SCHED_SPAWNED: u8 = 0x01;
    const TRACE_SCHED_UNBLOCKED: u8 = 0x02;
    const TRACE_SCHED_YIELDED: u8 = 0x03;
    const TRACE_SCHED_PREEMPTED: u8 = 0x04;
    const TRACE_SCHED_BLOCKED: u8 = 0x05;
    const TRACE_SCHED_EXITED: u8 = 0x06;

    const TRACE_SYSCALL_COMPLETED: u8 = 0x01;
    const TRACE_SYSCALL_YIELDED: u8 = 0x02;
    const TRACE_SYSCALL_BLOCKED: u8 = 0x03;
    const TRACE_SYSCALL_ERROR: u8 = 0x04;

    /// Get human-readable syscall name from ID
    fn syscall_name(id: u16) -> &'static str {
        match id {
            SYS_SPAWN => "sys_spawn",
            SYS_EXIT => "sys_exit",
            SYS_WAIT => "sys_wait",
            SYS_YIELD => "sys_yield",
            SYS_WRITE => "sys_write",
            SYS_READ => "sys_read",
            SYS_INPUT_MODE => "sys_input_mode",
            SYS_CONSOLE_MODE => "sys_console_mode",
            SYS_FS_LIST => "sys_fs_list",
            SYS_FS_OPEN => "sys_fs_open",
            SYS_FS_READ => "sys_fs_read",
            SYS_FS_CLOSE => "sys_fs_close",
            SYS_FS_WRITE => "sys_fs_write",
            SYS_DBG_LIST => "sys_dbg_list",
            SYS_DBG_REGS => "sys_dbg_regs",
            SYS_DBG_MEM_READ => "sys_dbg_mem_read",
            SYS_DBG_MEM_WRITE => "sys_dbg_mem_write",
            SYS_DBG_TRACE_READ => "sys_dbg_trace_read",
            SYS_PERF_MEM_STATS => "sys_perf_mem_stats",
            SYS_PERF_PROC_INFO => "sys_perf_proc_info",
            SYS_SET_TIMING => "sys_set_timing",
            _ => "unknown",
        }
    }

    /// Get human-readable error name from code
    fn error_name(code: u8) -> &'static str {
        match code {
            ERR_INVALID => "ERR_INVALID",
            ERR_IO => "ERR_IO",
            ERR_NOT_FOUND => "ERR_NOT_FOUND",
            ERR_NOT_DIR => "ERR_NOT_DIR",
            ERR_IS_DIR => "ERR_IS_DIR",
            ERR_NAME_TOO_LONG => "ERR_NAME_TOO_LONG",
            ERR_TOO_MANY_OPEN => "ERR_TOO_MANY_OPEN",
            ERR_PATH => "ERR_PATH",
            0x0A => "ERR_STACK",
            _ => "ERR_UNKNOWN",
        }
    }

    /// Log syscall error in JSON format to stderr (gated by CHIP8_SYSCALL_ERRORS env var)
    /// Escape a string for safe inclusion in JSON.
    /// Handles quotes, backslashes, control characters.
    fn escape_json_string(s: &str) -> String {
        s.chars()
            .flat_map(|c| match c {
                '"' => vec!['\\', '"'],
                '\\' => vec!['\\', '\\'],
                '\n' => vec!['\\', 'n'],
                '\r' => vec!['\\', 'r'],
                '\t' => vec!['\\', 't'],
                c if c.is_control() => format!("\\u{:04x}", c as u32).chars().collect(),
                c => vec![c],
            })
            .collect()
    }

    fn log_syscall_error(pid: u32, syscall_id: u16, error_code: u8, message: &str) {
        if std::env::var(SYSCALL_ERROR_LOG_ENV).is_err() {
            return;
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        eprintln!(
            "{{\"level\":\"error\",\"timestamp\":{},\"event\":\"syscall_error\",\"pid\":{},\"syscall_id\":\"0x{:04X}\",\"syscall_name\":\"{}\",\"error_code\":\"0x{:02X}\",\"error_name\":\"{}\",\"message\":\"{}\"}}",
            timestamp,
            pid,
            syscall_id,
            syscall_name(syscall_id),
            error_code,
            error_name(error_code),
            escape_json_string(message)
        );
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
    pub enum SchedulerEvent {
        Spawned,
        Unblocked,
        Yielded,
        Preempted,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum ScheduleReason {
        Yielded,
        Blocked,
        Exited,
        Preempted,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum ScheduleOutcome {
        Idle,
        Ran { pid: u32, reason: ScheduleReason },
    }

    pub trait SchedulerPolicy: Send {
        fn on_runnable(&mut self, pid: u32, event: SchedulerEvent);
        fn on_blocked(&mut self, pid: u32);
        fn on_exit(&mut self, pid: u32);
        fn next(&mut self) -> Option<u32>;
        #[allow(dead_code)]
        fn has_runnable(&self) -> bool;
    }

    #[derive(Default)]
    pub struct RoundRobinScheduler {
        queue: VecDeque<u32>,
        queued: HashSet<u32>,
    }

    impl RoundRobinScheduler {
        fn enqueue(&mut self, pid: u32) {
            if self.queued.insert(pid) {
                self.queue.push_back(pid);
            }
        }

        fn drop_pid(&mut self, pid: u32) {
            if self.queued.remove(&pid) {
                self.queue.retain(|&entry| entry != pid);
            }
        }
    }

    impl SchedulerPolicy for RoundRobinScheduler {
        fn on_runnable(&mut self, pid: u32, _event: SchedulerEvent) {
            self.enqueue(pid);
        }

        fn on_blocked(&mut self, pid: u32) {
            self.drop_pid(pid);
        }

        fn on_exit(&mut self, pid: u32) {
            self.drop_pid(pid);
        }

        fn next(&mut self) -> Option<u32> {
            while let Some(pid) = self.queue.pop_front() {
                if self.queued.remove(&pid) {
                    return Some(pid);
                }
            }
            None
        }

        fn has_runnable(&self) -> bool {
            self.queue.iter().any(|pid| self.queued.contains(pid))
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

    /// Per-process timing configuration for execution control.
    ///
    /// Allows different processes to run at different speeds:
    /// - Legacy CHIP-8 programs can run at authentic 60Hz with throttling
    /// - Modern programs (CLI, debugger) can run at full CPU speed
    /// - Timeslice controls preemption frequency for fair scheduling
    #[derive(Copy, Clone, Debug)]
    struct TimingConfig {
        /// Instructions per timeslice before preemption (0 = use kernel default)
        timeslice_steps: u32,
        /// Target instructions per second (0 = unlimited, run at full speed)
        /// Set to 3600 for authentic 60Hz CHIP-8 (60 Hz * 60 instructions/frame)
        target_ips: u32,
        /// Timer tick rate in Hz (0 = use kernel default of 60Hz)
        /// Legacy CHIP-8: 60Hz, Modern: higher for better granularity
        timer_hz: u16,
    }

    impl Default for TimingConfig {
        fn default() -> Self {
            TimingConfig {
                timeslice_steps: 0,  // Use kernel default
                target_ips: 0,       // Full speed
                timer_hz: 0,         // Use kernel default (60Hz)
            }
        }
    }

    struct ProcEntry {
        proc: Proc,
        context: Context,
        state: ProcState,
        exit_code: Option<u8>,
        waiting_for: Option<WaitTarget>,
        timing: TimingConfig,
        last_throttle: Instant,  // For target_ips throttling
    }

    pub struct ProcGuard<'a> {
        entry: &'a mut ProcEntry,
    }

    impl<'a> std::ops::Deref for ProcGuard<'a> {
        type Target = Proc;

        fn deref(&self) -> &Proc {
            &self.entry.proc
        }
    }

    impl<'a> std::ops::DerefMut for ProcGuard<'a> {
        fn deref_mut(&mut self) -> &mut Proc {
            &mut self.entry.proc
        }
    }

    impl<'a> Drop for ProcGuard<'a> {
        fn drop(&mut self) {
            self.entry.context = self.entry.proc.context();
        }
    }

    struct FdTable {
        fds: HashMap<u8, fs::File>,
        next_fd: u8,
    }

    pub struct Kernel {
        mem: Arc<Mutex<SharedMemory>>,
        syscalls: SyscallTable,
        procs: HashMap<u32, ProcEntry>,
        fd_tables: HashMap<u32, FdTable>,
        next_pid: u32,
        root_dir: PathBuf,
        input: VecDeque<u8>,
        pending_exit: HashMap<u32, u8>,
        pending_block: HashMap<u32, WaitTarget>,
        pending_timing: HashMap<u32, TimingConfig>,
        last_timer_tick: Instant,
        timeslice_steps: u32,
        scheduler: Box<dyn SchedulerPolicy>,
        trace: TraceBuffer,
    }

    struct TraceBuffer {
        records: Vec<[u8; TRACE_RECORD_SIZE]>,
        head: usize,
        tail: usize,
        len: usize,
    }

    impl TraceBuffer {
        fn new(capacity: usize) -> TraceBuffer {
            let cap = capacity.max(1);
            TraceBuffer {
                records: vec![[0u8; TRACE_RECORD_SIZE]; cap],
                head: 0,
                tail: 0,
                len: 0,
            }
        }

        fn push(&mut self, record: [u8; TRACE_RECORD_SIZE]) {
            let cap = self.records.len();
            if cap == 0 {
                return;
            }
            self.records[self.head] = record;
            if self.len == cap {
                self.tail = (self.tail + 1) % cap;
            } else {
                self.len += 1;
            }
            self.head = (self.head + 1) % cap;
        }

        fn pop_many(&mut self, max: usize) -> Vec<[u8; TRACE_RECORD_SIZE]> {
            let count = max.min(self.len);
            let mut out = Vec::with_capacity(count);
            for _ in 0..count {
                let record = self.records[self.tail];
                out.push(record);
                self.tail = (self.tail + 1) % self.records.len();
                self.len -= 1;
            }
            out
        }

        fn len(&self) -> usize {
            self.len
        }
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
                fd_tables: HashMap::new(),
                next_pid: 1,
                root_dir: root,
                input: VecDeque::new(),
                pending_exit: HashMap::new(),
                pending_block: HashMap::new(),
                pending_timing: HashMap::new(),
                last_timer_tick: Instant::now(),
                timeslice_steps: DEFAULT_TIMESLICE_STEPS,
                scheduler: Box::new(RoundRobinScheduler::default()),
                trace: TraceBuffer::new(TRACE_DEFAULT_CAPACITY),
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
            #[cfg(feature = "fs_write")]
            self.register_syscall(SYS_FS_WRITE, sys_fs_write)?;
            self.register_syscall(SYS_DBG_LIST, sys_dbg_list)?;
            self.register_syscall(SYS_DBG_REGS, sys_dbg_regs)?;
            self.register_syscall(SYS_DBG_MEM_READ, sys_dbg_mem_read)?;
            self.register_syscall(SYS_DBG_MEM_WRITE, sys_dbg_mem_write)?;
            self.register_syscall(SYS_DBG_TRACE_READ, sys_dbg_trace_read)?;
            self.register_syscall(SYS_PERF_MEM_STATS, sys_perf_mem_stats)?;
            self.register_syscall(SYS_PERF_PROC_INFO, sys_perf_proc_info)?;
            self.register_syscall(SYS_SET_TIMING, sys_set_timing)?;
            Ok(())
        }

        /// register a syscall handler on the shared registry.
        pub fn register_syscall<H>(&mut self, id: u16, handler: H) -> Result<(), Error>
        where
            H: Fn(&mut Kernel, u32, &mut Proc) -> SyscallOutcome + Send + Sync + 'static,
        {
            self.syscalls.register(id, handler)
        }

        fn trace_record(&mut self, kind: u8, op: u8, pid: u32, arg0: u16, arg1: u16) {
            let mut record = [0u8; TRACE_RECORD_SIZE];
            record[0] = kind;
            record[1] = op;
            let pid16 = (pid & 0xFFFF) as u16;
            record[2..4].copy_from_slice(&pid16.to_be_bytes());
            record[4..6].copy_from_slice(&arg0.to_be_bytes());
            record[6..8].copy_from_slice(&arg1.to_be_bytes());
            self.trace.push(record);
        }

        fn trace_sched(&mut self, pid: u32, event: u8) {
            self.trace_record(TRACE_KIND_SCHED, event, pid, 0, 0);
        }

        fn trace_syscall(&mut self, pid: u32, id: u16, outcome: u8, error_code: u8) {
            self.trace_record(TRACE_KIND_SYSCALL, outcome, pid, id, error_code as u16);
        }

        /// replace the scheduling policy and seed it with all runnable pids.
        #[allow(dead_code)]
        pub fn set_scheduler_policy(&mut self, mut policy: Box<dyn SchedulerPolicy>) {
            for (&pid, entry) in self.procs.iter() {
                if entry.state == ProcState::Running {
                    policy.on_runnable(pid, SchedulerEvent::Spawned);
                }
            }
            self.scheduler = policy;
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
            let context = proc.context();

            self.procs.insert(
                pid,
                ProcEntry {
                    proc,
                    context,
                    state: ProcState::Running,
                    exit_code: None,
                    waiting_for: None,
                    timing: TimingConfig::default(),
                    last_throttle: Instant::now(),
                },
            );
            self.fd_tables.insert(
                pid,
                FdTable {
                    fds: HashMap::new(),
                    next_fd: 1,
                },
            );
            self.scheduler.on_runnable(pid, SchedulerEvent::Spawned);
            self.trace_sched(pid, TRACE_SCHED_SPAWNED);
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

        /// set the number of instruction steps each proc gets per scheduling slice.
        #[allow(dead_code)]
        pub fn set_timeslice_steps(&mut self, steps: u32) {
            self.timeslice_steps = steps.max(1);
        }

        #[allow(dead_code)]
        /// access a process by pid for inspection (tests/tools).
        pub fn proc(&self, pid: u32) -> Option<&Proc> {
            self.procs.get(&pid).map(|entry| &entry.proc)
        }

        #[allow(dead_code)]
        /// access a process by pid for mutation (tests/debug only).
        pub fn proc_mut(&mut self, pid: u32) -> Option<ProcGuard<'_>> {
            self.procs
                .get_mut(&pid)
                .map(|entry| ProcGuard { entry })
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

            entry.proc.restore_context(&entry.context);
            let ticks = self.timer_ticks();
            let outcome = entry
                .proc
                .step(ticks, |id, proc| self.dispatch_syscall(pid, proc, id));
            entry.proc.display.present_if_due();
            if self.drain_display_input(&mut entry.proc) {
                self.unblock_readers();
            }

            self.apply_pending(pid, &mut entry, outcome);
            entry.context = entry.proc.context();
            self.procs.insert(pid, entry);
            Ok(outcome)
        }

        /// schedule a single runnable pid using the current policy.
        /// Run one scheduling cycle: select next runnable process and execute it.
        ///
        /// Scheduler state machine:
        /// 1. Poll for console input (may unblock waiting processes)
        /// 2. Select next runnable process (policy-driven: round-robin, priority, etc.)
        /// 3. Execute process until it yields, blocks, preempts, or exits
        /// 4. Update scheduler state and trace the transition
        /// 5. Return outcome (Ran or Idle)
        ///
        /// Preemption occurs when timeslice is exhausted. Blocking occurs on I/O waits.
        /// Yielding is cooperative (sys_yield). Exiting is terminal (sys_exit).
        pub fn schedule_once(&mut self) -> Result<ScheduleOutcome, Error> {
            self.poll_console_input();
            let Some(pid) = self.next_runnable_pid() else {
                return Ok(ScheduleOutcome::Idle);
            };

            let reason = self.run_proc_until_yield_or_block(pid)?;
            match reason {
                ScheduleReason::Yielded => {
                    self.trace_sched(pid, TRACE_SCHED_YIELDED);
                    self.scheduler.on_runnable(pid, SchedulerEvent::Yielded);
                }
                ScheduleReason::Preempted => {
                    self.trace_sched(pid, TRACE_SCHED_PREEMPTED);
                    self.scheduler.on_runnable(pid, SchedulerEvent::Preempted);
                }
                ScheduleReason::Blocked => {
                    self.trace_sched(pid, TRACE_SCHED_BLOCKED);
                    self.scheduler.on_blocked(pid);
                }
                ScheduleReason::Exited => {
                    self.trace_sched(pid, TRACE_SCHED_EXITED);
                    self.scheduler.on_exit(pid);
                }
            }
            Ok(ScheduleOutcome::Ran { pid, reason })
        }

        /// load a ROM into an existing process by pid.
        pub fn load_rom(&mut self, pid: u32, rom_path: &Path) -> Result<(), Error> {
            let entry = self
                .procs
                .get_mut(&pid)
                .ok_or_else(|| Error::new(ErrorKind::NotFound, "pid not found"))?;
            let rom_bytes = fs::read(rom_path)?;
            entry.proc.load_program_bytes(&rom_bytes)
        }

        /// run the cooperative scheduler until no runnable procs remain.
        pub fn run(&mut self) -> Result<(), Error> {
            loop {
                if let ScheduleOutcome::Ran { .. } = self.schedule_once()? {
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

                if entry.proc.console_mode == ConsoleMode::Display {
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
            let pid = proc.regs.V[0];
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
            proc.display.present_if_due();
            if std::env::var(DEBUG_INPUT_ENV).is_ok() {
                let bytes: Vec<String> = data.iter().map(|b| format!("{:02X}", b)).collect();
                let ascii = String::from_utf8_lossy(data);
                eprintln!("[input] pid={} len={} bytes=[{}] ascii={:?}",
                    pid,
                    data.len(),
                    bytes.join(" "),
                    ascii
                );
            }
        }

        fn drain_display_input(&mut self, proc: &mut Proc) -> bool {
            if proc.console_mode != ConsoleMode::Display {
                return false;
            }
            let data = proc.display.drain_text_input();
            if data.is_empty() {
                return false;
            }
            self.apply_console_input(proc, &data);
            true
        }

        fn run_proc_until_yield_or_block(&mut self, pid: u32) -> Result<ScheduleReason, Error> {
            // Get per-process timeslice (or fall back to kernel default)
            let entry_timing = self.procs.get(&pid)
                .map(|e| e.timing)
                .unwrap_or_default();
            let slice = if entry_timing.timeslice_steps > 0 {
                entry_timing.timeslice_steps
            } else {
                self.timeslice_steps.max(1)
            };

            let mut steps = 0u32;
            loop {
                if steps >= slice {
                    return Ok(ScheduleReason::Preempted);
                }
                steps = steps.saturating_add(1);
                let mut entry = self
                    .procs
                    .remove(&pid)
                    .ok_or_else(|| Error::new(ErrorKind::NotFound, "pid not found"))?;
                if entry.state != ProcState::Running {
                    self.procs.insert(pid, entry);
                    return Ok(ScheduleReason::Blocked);
                }

                // Apply per-instruction throttling if target_ips is set (legacy mode)
                if entry.timing.target_ips > 0 {
                    let target_interval = Duration::from_nanos(1_000_000_000 / entry.timing.target_ips as u64);
                    let elapsed = entry.last_throttle.elapsed();
                    if elapsed < target_interval {
                        thread::sleep(target_interval - elapsed);
                    }
                    entry.last_throttle = Instant::now();
                }

                entry.proc.restore_context(&entry.context);
                let ticks = self.timer_ticks();
                let outcome = entry
                    .proc
                    .step(ticks, |id, proc| self.dispatch_syscall(pid, proc, id));
                entry.proc.display.present_if_due();
                if self.drain_display_input(&mut entry.proc) {
                    self.unblock_readers();
                }

                self.apply_pending(pid, &mut entry, outcome);
                entry.context = entry.proc.context();
                let reason = match entry.state {
                    ProcState::Exited => Some(ScheduleReason::Exited),
                    ProcState::Blocked => Some(ScheduleReason::Blocked),
                    ProcState::Running => {
                        if outcome == SyscallOutcome::Yielded {
                            Some(ScheduleReason::Yielded)
                        } else {
                            None
                        }
                    }
                };
                self.procs.insert(pid, entry);
                if let Some(reason) = reason {
                    return Ok(reason);
                }
            }
        }

        fn next_runnable_pid(&mut self) -> Option<u32> {
            while let Some(pid) = self.scheduler.next() {
                if self.is_runnable(pid) {
                    return Some(pid);
                }
            }
            None
        }

        fn dispatch_syscall(&mut self, pid: u32, proc: &mut Proc, id: u16) -> Result<SyscallOutcome, Error> {
            let handler = self
                .syscalls
                .handler(id);
            let Some(handler) = handler else {
                if id != SYS_DBG_TRACE_READ {
                    self.trace_syscall(pid, id, TRACE_SYSCALL_ERROR, ERR_INVALID);
                }
                return Err(Error::new(ErrorKind::NotFound, "unknown syscall id"));
            };
            let outcome = handler(self, pid, proc);
            let trace_outcome = match outcome {
                SyscallOutcome::Completed => TRACE_SYSCALL_COMPLETED,
                SyscallOutcome::Yielded => TRACE_SYSCALL_YIELDED,
                SyscallOutcome::Blocked => TRACE_SYSCALL_BLOCKED,
            };
            if id != SYS_DBG_TRACE_READ {
                // Record error code from V[0] if VF=1 (error), otherwise 0 (success)
                let error_code = if proc.regs.V[0xF] == 1 { proc.regs.V[0] } else { 0 };
                self.trace_syscall(pid, id, trace_outcome, error_code);
            }
            Ok(outcome)
        }

        fn apply_pending(&mut self, pid: u32, entry: &mut ProcEntry, outcome: SyscallOutcome) {
            // Apply pending timing configuration if present
            if let Some(timing) = self.pending_timing.remove(&pid) {
                entry.timing = timing;
            }

            if let Some(code) = self.pending_exit.remove(&pid) {
                entry.state = ProcState::Exited;
                entry.exit_code = Some(code);
                entry.waiting_for = None;
                self.unblock_waiters(pid, code);
                self.fd_tables.remove(&pid);

                // Free allocated pages when process exits
                let page_table = entry.proc.page_table.clone();
                if let Ok(mut mem) = self.mem.lock() {
                    let _ = mem.munmap(&page_table);
                }
            } else if let Some(wait) = self.pending_block.remove(&pid) {
                entry.state = ProcState::Blocked;
                entry.waiting_for = Some(wait);
            } else if outcome == SyscallOutcome::Blocked {
                entry.state = ProcState::Blocked;
            }
        }

        fn timer_ticks(&mut self) -> u32 {
            let tick = Duration::from_micros(1_000_000 / 60);
            let elapsed = self.last_timer_tick.elapsed();
            if elapsed < tick {
                return 0;
            }

            let ticks = (elapsed.as_nanos() / tick.as_nanos()) as u32;
            self.last_timer_tick = self.last_timer_tick + (tick * ticks);
            ticks
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
            let mut unblocked = Vec::new();
            for (&entry_pid, entry) in self.procs.iter_mut() {
                if entry.state != ProcState::Blocked {
                    continue;
                }
                if let Some(WaitTarget::Pid(wait_pid)) = entry.waiting_for {
                    if wait_pid == waited_pid {
                        entry.proc.regs.V[0] = code;
                        entry.proc.regs.V[0xF] = 0;
                        entry.state = ProcState::Running;
                        entry.waiting_for = None;
                        entry.context = entry.proc.context();
                        unblocked.push(entry_pid);
                    }
                }
            }
            for pid in unblocked {
                self.trace_sched(pid, TRACE_SCHED_UNBLOCKED);
                self.scheduler.on_runnable(pid, SchedulerEvent::Unblocked);
            }
        }

        fn unblock_readers(&mut self) {
            // console-backed readers: each proc has its own input queue.
            let mut unblocked = Vec::new();
            for (&pid, entry) in self.procs.iter_mut() {
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
                entry.context = entry.proc.context();
                unblocked.push(pid);
            }

            for (&pid, entry) in self.procs.iter_mut() {
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
                entry.context = entry.proc.context();
                unblocked.push(pid);
            }

            if self.input.is_empty() {
                for pid in unblocked {
                    self.trace_sched(pid, TRACE_SCHED_UNBLOCKED);
                    self.scheduler.on_runnable(pid, SchedulerEvent::Unblocked);
                }
                return;
            }

            let (procs, input) = (&mut self.procs, &mut self.input);

            // host-backed readers: line mode first, then byte mode.
            for (&pid, entry) in procs.iter_mut() {
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
                entry.context = entry.proc.context();
                unblocked.push(pid);
            }

            for (&pid, entry) in procs.iter_mut() {
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
                entry.context = entry.proc.context();
                unblocked.push(pid);
            }

            for pid in unblocked {
                self.trace_sched(pid, TRACE_SCHED_UNBLOCKED);
                self.scheduler.on_runnable(pid, SchedulerEvent::Unblocked);
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

        fn fs_error_to_code(err: FsError) -> u8 {
            match err {
                FsError::Invalid => ERR_INVALID,
                FsError::Io => ERR_IO,
                FsError::NotFound => ERR_NOT_FOUND,
                FsError::NotDir => ERR_NOT_DIR,
                FsError::IsDir => ERR_IS_DIR,
                FsError::NameTooLong => ERR_NAME_TOO_LONG,
                FsError::TooManyOpen => ERR_TOO_MANY_OPEN,
                FsError::Path => ERR_PATH,
            }
        }

        fn fs_list_entries(&self, path: &str, max_entries: usize) -> Result<Vec<FsEntry>, FsError> {
            let dir_path = self
                .resolve_fs_path(path)
                .map_err(|err| if err.kind() == ErrorKind::NotFound { FsError::NotFound } else { FsError::Path })?;

            let meta = fs::metadata(&dir_path).map_err(|_| FsError::NotFound)?;
            if !meta.is_dir() {
                return Err(FsError::NotDir);
            }

            let entries = fs::read_dir(&dir_path).map_err(|_| FsError::Io)?;
            let mut out = Vec::new();
            for entry in entries {
                if out.len() >= max_entries {
                    break;
                }
                let entry = match entry {
                    Ok(val) => val,
                    Err(_) => continue,
                };
                let name = entry.file_name().to_string_lossy().to_string();
                if name.len() > MAX_FILENAME_LEN {
                    return Err(FsError::NameTooLong);
                }
                let meta = match entry.metadata() {
                    Ok(val) => val,
                    Err(_) => continue,
                };
                let kind = if meta.is_dir() { FsEntryKind::Dir } else { FsEntryKind::File };
                let size = if meta.is_file() { meta.len() as u32 } else { 0u32 };
                out.push(FsEntry { name, kind, size });
            }
            Ok(out)
        }

        fn fs_open_path(&mut self, pid: u32, path: &str, flags: u16) -> Result<u8, FsError> {
            let file_path = self
                .resolve_fs_path(path)
                .map_err(|err| if err.kind() == ErrorKind::NotFound { FsError::NotFound } else { FsError::Path })?;

            let meta = fs::metadata(&file_path).map_err(|_| FsError::NotFound)?;
            if meta.is_dir() {
                return Err(FsError::IsDir);
            }
            if meta.len() > MAX_FILE_SIZE {
                return Err(FsError::Io);
            }

            let table = self.fd_tables.get_mut(&pid).ok_or(FsError::NotFound)?;
            if table.fds.len() >= MAX_OPEN_FILES {
                return Err(FsError::TooManyOpen);
            }

            // flags bit 0: write mode (0 = read-only, 1 = read-write)
            // Only respect write flag if fs_write feature is enabled
            #[cfg(feature = "fs_write")]
            let file = if flags & 0x01 != 0 {
                fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(&file_path)
                    .map_err(|_| FsError::Io)?
            } else {
                fs::File::open(&file_path).map_err(|_| FsError::Io)?
            };

            #[cfg(not(feature = "fs_write"))]
            let file = fs::File::open(&file_path).map_err(|_| FsError::Io)?;

            let mut fd = table.next_fd;
            for _ in 0..=u8::MAX {
                if fd == 0 {
                    fd = 1;
                }
                if !table.fds.contains_key(&fd) {
                    break;
                }
                fd = fd.wrapping_add(1);
            }
            if table.fds.contains_key(&fd) {
                return Err(FsError::TooManyOpen);
            }

            table.fds.insert(fd, file);
            table.next_fd = fd.wrapping_add(1);
            Ok(fd)
        }

        fn fs_read_fd(&mut self, pid: u32, fd: u8, len: usize) -> Result<Vec<u8>, FsError> {
            let table = self.fd_tables.get_mut(&pid).ok_or(FsError::NotFound)?;
            let file = table.fds.get_mut(&fd).ok_or(FsError::NotFound)?;

            let max_len = len.min(0xFF);
            let mut data = vec![0u8; max_len];
            let read = file.read(&mut data).map_err(|_| FsError::Io)?;
            data.truncate(read);
            Ok(data)
        }

        #[cfg(feature = "fs_write")]
        fn fs_write_fd(&mut self, pid: u32, fd: u8, data: &[u8]) -> Result<usize, FsError> {
            use std::io::Write;
            let table = self.fd_tables.get_mut(&pid).ok_or(FsError::NotFound)?;
            let file = table.fds.get_mut(&fd).ok_or(FsError::NotFound)?;

            let written = file.write(data).map_err(|_| FsError::Io)?;
            file.flush().map_err(|_| FsError::Io)?;
            Ok(written)
        }

        fn fs_close_fd(&mut self, pid: u32, fd: u8) -> Result<(), FsError> {
            let table = self.fd_tables.get_mut(&pid).ok_or(FsError::NotFound)?;
            if table.fds.remove(&fd).is_none() {
                return Err(FsError::NotFound);
            }
            Ok(())
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

    fn sys_spawn(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let name_ptr = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_SPAWN, ERR_INVALID, "syscall frame too small for name_ptr");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let name_len = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_SPAWN, ERR_INVALID, "syscall frame too small for name_len");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let pages = Kernel::syscall_arg(proc, 2).unwrap_or(1);
        let name_bytes = match proc.read_bytes(name_ptr as u32, name_len as usize) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_SPAWN, ERR_INVALID, "name buffer read failed");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let rom_name = String::from_utf8_lossy(&name_bytes).to_string();
        let path = match kernel.resolve_rom_path(&rom_name) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_SPAWN, ERR_IO, "ROM path resolution failed");
                proc.regs.V[0] = ERR_IO;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let display = match DisplayWindow::from_env() {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_SPAWN, ERR_IO, "display initialization failed");
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
                log_syscall_error(pid, SYS_SPAWN, ERR_IO, "process spawn failed");
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
                log_syscall_error(pid, SYS_WAIT, ERR_INVALID, "syscall frame too small for target pid");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let Some(target_entry) = kernel.procs.get(&target) else {
            log_syscall_error(pid, SYS_WAIT, ERR_INVALID, "target pid not found");
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

    fn sys_write(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let buf = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_WRITE, ERR_INVALID, "syscall frame too small for buf");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let len = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_WRITE, ERR_INVALID, "syscall frame too small for len");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let data = match proc.read_bytes(buf as u32, len as usize) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_WRITE, ERR_INVALID, "buffer read failed (out of bounds)");
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

        // Use InputDevice trait for host output (enables QEMU/embedded portability)
        match kernel.write_output(&data) {
            Ok(written) => {
                proc.regs.V[0] = (written.min(0xFF)) as u8;
                proc.regs.V[0xF] = 0;
            }
            Err(_) => {
                log_syscall_error(pid, SYS_WRITE, ERR_IO, "host output write failed");
                proc.regs.V[0] = ERR_IO;
                proc.regs.V[0xF] = 1;
            }
        }
        SyscallOutcome::Completed
    }

    fn sys_read(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let buf = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_READ, ERR_INVALID, "syscall frame too small for buf");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let len = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_READ, ERR_INVALID, "syscall frame too small for len");
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
                        log_syscall_error(pid, SYS_READ, ERR_INVALID, "buffer write failed (out of bounds)");
                        proc.regs.V[0] = ERR_INVALID;
                        proc.regs.V[0xF] = 1;
                    } else {
                        proc.regs.V[0] = count.min(0xFF) as u8;
                        proc.regs.V[0xF] = 0;
                    }
                    if std::env::var(DEBUG_INPUT_ENV).is_ok() {
                        let bytes: Vec<String> = data.iter().map(|b| format!("{:02X}", b)).collect();
                        let ascii = String::from_utf8_lossy(&data);
                        eprintln!("[read] pid={} mode=line count={} bytes=[{}] ascii={:?}",
                            pid,
                            count,
                            bytes.join(" "),
                            ascii
                        );
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
                    log_syscall_error(pid, SYS_READ, ERR_INVALID, "buffer write failed (out of bounds)");
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
                        log_syscall_error(pid, SYS_READ, ERR_INVALID, "buffer write failed (out of bounds)");
                        proc.regs.V[0] = ERR_INVALID;
                        proc.regs.V[0xF] = 1;
                    } else {
                        proc.regs.V[0] = count.min(0xFF) as u8;
                        proc.regs.V[0xF] = 0;
                    }
                    if std::env::var(DEBUG_INPUT_ENV).is_ok() {
                        let bytes: Vec<String> = data.iter().map(|b| format!("{:02X}", b)).collect();
                        let ascii = String::from_utf8_lossy(&data);
                        eprintln!("[read] pid={} mode=byte count={} bytes=[{}] ascii={:?}",
                            pid,
                            count,
                            bytes.join(" "),
                            ascii
                        );
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
                    log_syscall_error(pid, SYS_READ, ERR_INVALID, "buffer write failed (out of bounds)");
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

    fn sys_input_mode(_kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let mode = match Kernel::syscall_arg(proc, 0) {
            Ok(0) => InputMode::Line,
            Ok(1) => InputMode::Byte,
            Ok(_) => {
                log_syscall_error(pid, SYS_INPUT_MODE, ERR_INVALID, "invalid mode value (must be 0 or 1)");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
            Err(_) => {
                log_syscall_error(pid, SYS_INPUT_MODE, ERR_INVALID, "syscall frame too small for mode");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        proc.input_mode = mode;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_console_mode(_kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let mode = match Kernel::syscall_arg(proc, 0) {
            Ok(0) => ConsoleMode::Host,
            Ok(1) => ConsoleMode::Display,
            Ok(_) => {
                log_syscall_error(pid, SYS_CONSOLE_MODE, ERR_INVALID, "invalid mode value (must be 0 or 1)");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
            Err(_) => {
                log_syscall_error(pid, SYS_CONSOLE_MODE, ERR_INVALID, "syscall frame too small for mode");
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

    fn sys_fs_list(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let path_ptr = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_LIST, ERR_INVALID, "syscall frame too small for path_ptr");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let path_len = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_LIST, ERR_INVALID, "syscall frame too small for path_len");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let out_ptr = match Kernel::syscall_arg(proc, 2) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_LIST, ERR_INVALID, "syscall frame too small for out_ptr");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let max_entries = match Kernel::syscall_arg(proc, 3) {
            Ok(val) => val as usize,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_LIST, ERR_INVALID, "syscall frame too small for max_entries");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let path_bytes = match proc.read_bytes(path_ptr as u32, path_len as usize) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_LIST, ERR_INVALID, "path read failed (out of bounds)");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let path_str = String::from_utf8_lossy(&path_bytes).to_string();
        let entries = match kernel.fs_list_entries(&path_str, max_entries) {
            Ok(val) => val,
            Err(err) => {
                let code = Kernel::fs_error_to_code(err);
                log_syscall_error(pid, SYS_FS_LIST, code, "directory listing failed");
                proc.regs.V[0] = code;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        for (count, entry) in entries.iter().enumerate() {
            let kind = match entry.kind {
                FsEntryKind::Dir => 1u8,
                FsEntryKind::File => 0u8,
            };

            let mut record = Vec::with_capacity(DIR_ENTRY_SIZE);
            record.push(entry.name.len() as u8);
            record.extend_from_slice(entry.name.as_bytes());
            if entry.name.len() < MAX_FILENAME_LEN {
                record.extend(std::iter::repeat(0u8).take(MAX_FILENAME_LEN - entry.name.len()));
            }
            record.push(kind);
            record.extend_from_slice(&entry.size.to_be_bytes());

            let addr = out_ptr as u32 + (count * DIR_ENTRY_SIZE) as u32;
            if proc.write_bytes(addr, &record).is_err() {
                log_syscall_error(pid, SYS_FS_LIST, ERR_INVALID, "entry write failed (out of bounds)");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        }

        proc.regs.V[0] = entries.len().min(0xFF) as u8;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_fs_open(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let path_ptr = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_OPEN, ERR_INVALID, "syscall frame too small for path_ptr");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let path_len = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_OPEN, ERR_INVALID, "syscall frame too small for path_len");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let flags = Kernel::syscall_arg(proc, 2).unwrap_or(0);

        let path_bytes = match proc.read_bytes(path_ptr as u32, path_len as usize) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_OPEN, ERR_INVALID, "path read failed (out of bounds)");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let path_str = String::from_utf8_lossy(&path_bytes).to_string();
        let fd = match kernel.fs_open_path(pid, &path_str, flags) {
            Ok(val) => val,
            Err(err) => {
                let code = Kernel::fs_error_to_code(err);
                log_syscall_error(pid, SYS_FS_OPEN, code, "file open failed");
                proc.regs.V[0] = code;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        proc.regs.V[0] = fd;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_fs_read(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let fd = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val as u8,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_READ, ERR_INVALID, "syscall frame too small for fd");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let buf = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_READ, ERR_INVALID, "syscall frame too small for buf");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let len = match Kernel::syscall_arg(proc, 2) {
            Ok(val) => val as usize,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_READ, ERR_INVALID, "syscall frame too small for len");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let data = match kernel.fs_read_fd(pid, fd, len) {
            Ok(val) => val,
            Err(err) => {
                let code = Kernel::fs_error_to_code(err);
                log_syscall_error(pid, SYS_FS_READ, code, "file read failed");
                proc.regs.V[0] = code;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        if proc.write_bytes(buf as u32, &data).is_err() {
            log_syscall_error(pid, SYS_FS_READ, ERR_INVALID, "buffer write failed (out of bounds)");
            proc.regs.V[0] = ERR_INVALID;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }

        proc.regs.V[0] = data.len().min(0xFF) as u8;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_fs_close(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let fd = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val as u8,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_CLOSE, ERR_INVALID, "syscall frame too small for fd");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        if let Err(err) = kernel.fs_close_fd(pid, fd) {
            let code = Kernel::fs_error_to_code(err);
            log_syscall_error(pid, SYS_FS_CLOSE, code, "file close failed");
            proc.regs.V[0] = code;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    #[cfg(feature = "fs_write")]
    fn sys_fs_write(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let fd = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val as u8,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_WRITE, ERR_INVALID, "syscall frame too small for fd");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let buf = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_WRITE, ERR_INVALID, "syscall frame too small for buf");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let len = match Kernel::syscall_arg(proc, 2) {
            Ok(val) => val as usize,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_WRITE, ERR_INVALID, "syscall frame too small for len");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let data = match proc.read_bytes(buf as u32, len) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_FS_WRITE, ERR_INVALID, "buffer read failed (out of bounds)");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        match kernel.fs_write_fd(pid, fd, &data) {
            Ok(written) => {
                proc.regs.V[0] = written.min(0xFF) as u8;
                proc.regs.V[0xF] = 0;
            }
            Err(err) => {
                let code = Kernel::fs_error_to_code(err);
                log_syscall_error(pid, SYS_FS_WRITE, code, "file write failed");
                proc.regs.V[0] = code;
                proc.regs.V[0xF] = 1;
            }
        }
        SyscallOutcome::Completed
    }

    fn sys_dbg_list(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let out_ptr = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_LIST, ERR_INVALID, "syscall frame too small for out_ptr");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let max_entries = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val as usize,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_LIST, ERR_INVALID, "syscall frame too small for max_entries");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let mut pids: Vec<u32> = kernel.procs.keys().copied().collect();
        pids.sort_unstable();
        let mut count = 0usize;

        for pid in pids.into_iter().take(max_entries) {
            let entry = match kernel.procs.get(&pid) {
                Some(val) => val,
                None => continue,
            };
            let state = match entry.state {
                ProcState::Running => 0u8,
                ProcState::Blocked => 1u8,
                ProcState::Exited => 2u8,
            };
            let exit_code = entry.exit_code.unwrap_or(0);
            let mut record = Vec::with_capacity(DBG_PROC_RECORD_SIZE);
            record.extend_from_slice(&pid.to_be_bytes());
            record.push(state);
            record.push(exit_code);
            record.extend_from_slice(&0u16.to_be_bytes());

            let addr = out_ptr as u32 + (count * DBG_PROC_RECORD_SIZE) as u32;
            if proc.write_bytes(addr, &record).is_err() {
                log_syscall_error(pid, SYS_DBG_LIST, ERR_INVALID, "record write failed (out of bounds)");
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

    fn sys_dbg_regs(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let target_pid = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val as u32,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_REGS, ERR_INVALID, "syscall frame too small for target_pid");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let out_ptr = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_REGS, ERR_INVALID, "syscall frame too small for out_ptr");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let entry = match kernel.procs.get_mut(&target_pid) {
            Some(val) => val,
            None => {
                log_syscall_error(pid, SYS_DBG_REGS, ERR_NOT_FOUND, "target process not found");
                proc.regs.V[0] = ERR_NOT_FOUND;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let regs = &entry.proc.regs;
        let mut record = Vec::with_capacity(DBG_REGS_SIZE);
        record.extend_from_slice(&regs.PC.to_be_bytes());
        record.extend_from_slice(&regs.SP.to_be_bytes());
        record.extend_from_slice(&regs.I.to_be_bytes());
        record.extend_from_slice(&regs.V);
        record.push(regs.DT);
        record.push(regs.ST);

        if proc.write_bytes(out_ptr as u32, &record).is_err() {
            log_syscall_error(pid, SYS_DBG_REGS, ERR_INVALID, "record write failed (out of bounds)");
            proc.regs.V[0] = ERR_INVALID;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }

        proc.regs.V[0] = (record.len().min(0xFF)) as u8;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_dbg_mem_read(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let target_pid = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val as u32,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_MEM_READ, ERR_INVALID, "syscall frame too small for target_pid");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let addr = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val as u32,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_MEM_READ, ERR_INVALID, "syscall frame too small for addr");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let len = match Kernel::syscall_arg(proc, 2) {
            Ok(val) => val as usize,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_MEM_READ, ERR_INVALID, "syscall frame too small for len");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let out_ptr = match Kernel::syscall_arg(proc, 3) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_MEM_READ, ERR_INVALID, "syscall frame too small for out_ptr");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let entry = match kernel.procs.get_mut(&target_pid) {
            Some(val) => val,
            None => {
                log_syscall_error(pid, SYS_DBG_MEM_READ, ERR_NOT_FOUND, "target process not found");
                proc.regs.V[0] = ERR_NOT_FOUND;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let data = match entry.proc.read_bytes(addr, len) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_MEM_READ, ERR_INVALID, "target memory read failed (out of bounds)");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        if proc.write_bytes(out_ptr as u32, &data).is_err() {
            log_syscall_error(pid, SYS_DBG_MEM_READ, ERR_INVALID, "buffer write failed (out of bounds)");
            proc.regs.V[0] = ERR_INVALID;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }

        proc.regs.V[0] = data.len().min(0xFF) as u8;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_dbg_mem_write(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let target_pid = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val as u32,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_MEM_WRITE, ERR_INVALID, "syscall frame too small for target_pid");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let addr = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val as u32,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_MEM_WRITE, ERR_INVALID, "syscall frame too small for addr");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let len = match Kernel::syscall_arg(proc, 2) {
            Ok(val) => val as usize,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_MEM_WRITE, ERR_INVALID, "syscall frame too small for len");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let in_ptr = match Kernel::syscall_arg(proc, 3) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_MEM_WRITE, ERR_INVALID, "syscall frame too small for in_ptr");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let entry = match kernel.procs.get_mut(&target_pid) {
            Some(val) => val,
            None => {
                log_syscall_error(pid, SYS_DBG_MEM_WRITE, ERR_NOT_FOUND, "target process not found");
                proc.regs.V[0] = ERR_NOT_FOUND;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let data = match proc.read_bytes(in_ptr as u32, len) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_MEM_WRITE, ERR_INVALID, "buffer read failed (out of bounds)");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        if entry.proc.write_bytes(addr, &data).is_err() {
            log_syscall_error(pid, SYS_DBG_MEM_WRITE, ERR_INVALID, "target memory write failed (out of bounds)");
            proc.regs.V[0] = ERR_INVALID;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }

        proc.regs.V[0] = data.len().min(0xFF) as u8;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_dbg_trace_read(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let out_ptr = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_TRACE_READ, ERR_INVALID, "syscall frame too small for out_ptr");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let max_records = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val as usize,
            Err(_) => {
                log_syscall_error(pid, SYS_DBG_TRACE_READ, ERR_INVALID, "syscall frame too small for max_records");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let count = max_records.min(kernel.trace.len());
        let records = kernel.trace.pop_many(count);
        if records.is_empty() {
            proc.regs.V[0] = 0;
            proc.regs.V[0xF] = 0;
            return SyscallOutcome::Completed;
        }

        let mut data = Vec::with_capacity(records.len() * TRACE_RECORD_SIZE);
        for record in records {
            data.extend_from_slice(&record);
        }

        if proc.write_bytes(out_ptr as u32, &data).is_err() {
            log_syscall_error(pid, SYS_DBG_TRACE_READ, ERR_INVALID, "buffer write failed (out of bounds)");
            proc.regs.V[0] = ERR_INVALID;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }

        proc.regs.V[0] = count.min(0xFF) as u8;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_perf_mem_stats(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let out_ptr = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_PERF_MEM_STATS, ERR_INVALID, "syscall frame too small for out_ptr");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let mem = match kernel.mem.lock() {
            Ok(mem) => mem,
            Err(_) => {
                log_syscall_error(pid, SYS_PERF_MEM_STATS, ERR_INVALID, "failed to lock shared memory");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        // Calculate stats
        const PHYS_PAGE_COUNT: u16 = 256;
        let total_pages = PHYS_PAGE_COUNT;

        // Count used pages by checking bitmap
        let used_pages = mem.used_pages() as u16;

        // Count free regions (fragmentation indicator)
        let free_regions = mem.free_regions() as u16;

        // Build stats struct (10 bytes)
        let mut stats = Vec::with_capacity(10);
        stats.extend_from_slice(&total_pages.to_be_bytes());
        stats.extend_from_slice(&used_pages.to_be_bytes());
        stats.extend_from_slice(&free_regions.to_be_bytes());
        stats.extend_from_slice(&[0u8; 4]); // Reserved

        drop(mem); // Release the lock before writing to proc memory

        if proc.write_bytes(out_ptr as u32, &stats).is_err() {
            log_syscall_error(pid, SYS_PERF_MEM_STATS, ERR_INVALID, "stats write failed (out of bounds)");
            proc.regs.V[0] = ERR_INVALID;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }

        proc.regs.V[0] = 0;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    fn sys_perf_proc_info(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let target_pid = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val as u32,
            Err(_) => {
                log_syscall_error(pid, SYS_PERF_PROC_INFO, ERR_INVALID, "syscall frame too small for target_pid");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };
        let out_ptr = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_PERF_PROC_INFO, ERR_INVALID, "syscall frame too small for out_ptr");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        // If target_pid is 0, use calling process's PID
        let actual_pid = if target_pid == 0 { pid } else { target_pid };

        let entry = match kernel.procs.get(&actual_pid) {
            Some(val) => val,
            None => {
                log_syscall_error(pid, SYS_PERF_PROC_INFO, ERR_NOT_FOUND, "target process not found");
                proc.regs.V[0] = ERR_NOT_FOUND;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        // Determine state: 0=ready, 1=blocked, 2=exited
        let state = match entry.state {
            ProcState::Running => 0u16,
            ProcState::Blocked => 1u16,
            ProcState::Exited => 2u16,
        };

        // Get process information
        let vm_pages = entry.proc.page_table.len() as u16;
        let stack_ptr = entry.proc.regs.SP;
        let pc = entry.proc.regs.PC;
        let input_mode = match entry.proc.input_mode {
            InputMode::Line => 0u16,
            InputMode::Byte => 1u16,
        };
        let console_mode = match entry.proc.console_mode {
            ConsoleMode::Display => 0u16,
            ConsoleMode::Host => 1u16,
        };

        // Build process info struct (16 bytes)
        let mut info = Vec::with_capacity(16);
        info.extend_from_slice(&(actual_pid as u16).to_be_bytes());
        info.extend_from_slice(&state.to_be_bytes());
        info.extend_from_slice(&vm_pages.to_be_bytes());
        info.extend_from_slice(&stack_ptr.to_be_bytes());
        info.extend_from_slice(&pc.to_be_bytes());
        info.extend_from_slice(&input_mode.to_be_bytes());
        info.extend_from_slice(&console_mode.to_be_bytes());
        info.extend_from_slice(&[0u8; 2]); // Reserved

        if proc.write_bytes(out_ptr as u32, &info).is_err() {
            log_syscall_error(pid, SYS_PERF_PROC_INFO, ERR_INVALID, "info write failed (out of bounds)");
            proc.regs.V[0] = ERR_INVALID;
            proc.regs.V[0xF] = 1;
            return SyscallOutcome::Completed;
        }

        proc.regs.V[0] = 1;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    /// SYS_SET_TIMING (0x0142): Configure per-process timing
    ///
    /// Allows processes to control their execution speed and scheduling behavior.
    /// Legacy CHIP-8 programs can run at authentic 60Hz, while modern programs
    /// (CLI, debugger) can run at full speed.
    ///
    /// Frame structure (8 bytes):
    ///   - len: 1 byte (0x08)
    ///   - syscall_id: 2 bytes (0x0142)
    ///   - target_pid: 2 bytes (0 = self)
    ///   - timeslice_steps_high: 1 byte
    ///   - timeslice_steps_low: 1 byte (combined: 0 = use kernel default)
    ///   - target_ips_high: 1 byte
    ///   - target_ips_low: 1 byte (combined: 0 = unlimited)
    ///   - timer_hz_high: 1 byte
    ///   - timer_hz_low: 1 byte (combined: 0 = use kernel default 60Hz)
    ///
    /// Error codes: ERR_INVALID, ERR_NOT_FOUND
    fn sys_set_timing(kernel: &mut Kernel, pid: u32, proc: &mut Proc) -> SyscallOutcome {
        let target_pid = match Kernel::syscall_arg(proc, 0) {
            Ok(val) => val as u32,
            Err(_) => {
                log_syscall_error(pid, SYS_SET_TIMING, ERR_INVALID, "syscall frame too small for target_pid");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let timeslice_steps = match Kernel::syscall_arg(proc, 1) {
            Ok(val) => val as u32,
            Err(_) => {
                log_syscall_error(pid, SYS_SET_TIMING, ERR_INVALID, "syscall frame too small for timeslice_steps");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let target_ips = match Kernel::syscall_arg(proc, 2) {
            Ok(val) => val as u32,
            Err(_) => {
                log_syscall_error(pid, SYS_SET_TIMING, ERR_INVALID, "syscall frame too small for target_ips");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        let timer_hz = match Kernel::syscall_arg(proc, 3) {
            Ok(val) => val,
            Err(_) => {
                log_syscall_error(pid, SYS_SET_TIMING, ERR_INVALID, "syscall frame too small for timer_hz");
                proc.regs.V[0] = ERR_INVALID;
                proc.regs.V[0xF] = 1;
                return SyscallOutcome::Completed;
            }
        };

        // If target_pid is 0, use calling process's PID
        let actual_pid = if target_pid == 0 { pid } else { target_pid };

        // Create timing configuration
        let timing = TimingConfig {
            timeslice_steps,
            target_ips,
            timer_hz,
        };

        // If configuring self, use pending_timing (proc is not in kernel.procs during syscall)
        if actual_pid == pid {
            kernel.pending_timing.insert(pid, timing);
        } else {
            // Configuring another process - look it up directly
            let entry = match kernel.procs.get_mut(&actual_pid) {
                Some(val) => val,
                None => {
                    log_syscall_error(pid, SYS_SET_TIMING, ERR_NOT_FOUND, "target process not found");
                    proc.regs.V[0] = ERR_NOT_FOUND;
                    proc.regs.V[0xF] = 1;
                    return SyscallOutcome::Completed;
                }
            };
            entry.timing = timing;
        }

        proc.regs.V[0] = 1;
        proc.regs.V[0xF] = 0;
        SyscallOutcome::Completed
    }

    impl InputDevice for Kernel {
        fn push_input(&mut self, data: &[u8]) {
            Kernel::push_input(self, data);
        }

        fn blocking_read_line(&mut self) -> Result<(), InputError> {
            self.blocking_read_line_from_stdin().map_err(|_| InputError::Io)
        }

        fn blocking_read_byte(&mut self) -> Result<(), InputError> {
            self.blocking_read_byte_from_stdin().map_err(|_| InputError::Io)
        }

        fn write_output(&mut self, data: &[u8]) -> Result<usize, InputError> {
            use std::io::Write;
            let mut stdout = io::stdout();
            stdout.write_all(data).map_err(|_| InputError::Io)?;
            stdout.flush().map_err(|_| InputError::Io)?;
            Ok(data.len())
        }
    }

    impl FsDevice for Kernel {
        fn list(&mut self, path: &str, max_entries: usize) -> Result<Vec<FsEntry>, FsError> {
            self.fs_list_entries(path, max_entries)
        }

        fn open(&mut self, pid: u32, path: &str) -> Result<u8, FsError> {
            self.fs_open_path(pid, path, 0)  // Default to read-only for trait impl
        }

        fn read(&mut self, pid: u32, fd: u8, len: usize) -> Result<Vec<u8>, FsError> {
            self.fs_read_fd(pid, fd, len)
        }

        fn close(&mut self, pid: u32, fd: u8) -> Result<(), FsError> {
            self.fs_close_fd(pid, fd)
        }
    }
}
