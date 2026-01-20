pub mod kernel {
    use std::collections::HashMap;
    use std::io::{Error, ErrorKind};
    use std::sync::{Arc, Mutex};

    use crate::display::display::DisplayWindow;
    use crate::proc::proc::Proc;
    use crate::shared_memory::shared_memory::SharedMemory;

    // syscall handlers return whether the caller should advance PC.
    pub enum SyscallOutcome {
        Completed,
        Blocked,
    }

    pub type SyscallHandler = fn(&mut Proc) -> SyscallOutcome;

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
        /// Example:
        /// ```
        /// use chip8_runtime::kernel::kernel::SyscallOutcome;
        /// use chip8_runtime::proc::proc::Proc;
        ///
        /// fn sys_ping(_proc: &mut Proc) -> SyscallOutcome { SyscallOutcome::Completed }
        /// let mut table = chip8_runtime::kernel::kernel::SyscallTable::new();
        /// table.register(0x0100, sys_ping).unwrap();
        /// ```
        pub fn register(&mut self, id: u16, handler: SyscallHandler) -> Result<(), Error> {
            if !(0x0100..0x0200).contains(&id) {
                return Err(Error::new(ErrorKind::InvalidInput, "syscall id out of range"));
            }
            self.handlers.insert(id, handler);
            Ok(())
        }

        /// look up a handler by syscall ID without executing it.
        /// Example: `if let Some(handler) = table.handler(0x0100) { ... }`
        pub fn handler(&self, id: u16) -> Option<SyscallHandler> {
            self.handlers.get(&id).copied()
        }
    }

    pub struct Kernel {
        mem: Arc<Mutex<SharedMemory>>,
        syscalls: Arc<Mutex<SyscallTable>>,
        procs: HashMap<u32, Proc>,
        next_pid: u32,
    }

    impl Kernel {
        /// build a kernel with shared memory and an empty syscall registry.
        /// Example: `let kernel = Kernel::new(shared_mem);`
        pub fn new(mem: Arc<Mutex<SharedMemory>>) -> Kernel {
            Kernel {
                mem,
                syscalls: Arc::new(Mutex::new(SyscallTable::new())),
                procs: HashMap::new(),
                next_pid: 1,
            }
        }

        /// register a syscall handler on the shared registry.
        /// Example: `kernel.register_syscall(0x0101, sys_spawn)?;`
        pub fn register_syscall(&mut self, id: u16, handler: SyscallHandler) -> Result<(), Error> {
            self.syscalls
                .lock()
                .unwrap()
                .register(id, handler)
        }

        /// create a new Proc bound to this kernel's shared memory and syscalls.
        /// Example: `let pid = kernel.spawn_proc(DisplayWindow::new()?, 1)?;`
        pub fn spawn_proc(&mut self, display: DisplayWindow, pages: u16) -> Result<u32, Error> {
            let pid = self.next_pid;
            self.next_pid = self.next_pid.wrapping_add(1);

            let proc = Proc::new_with_display_and_pages(
                Arc::clone(&self.mem),
                Arc::clone(&self.syscalls),
                display,
                pages,
            )?;

            self.procs.insert(pid, proc);
            Ok(pid)
        }

        /// access a mutable Proc by PID for scheduling or inspection.
        /// Example: `if let Some(proc) = kernel.proc_mut(pid) { proc.step(); }`
        pub fn proc_mut(&mut self, pid: u32) -> Option<&mut Proc> {
            self.procs.get_mut(&pid)
        }

        /// expose the shared syscall table for advanced registration.
        /// Example: `let syscalls = kernel.syscall_table();`
        pub fn syscall_table(&self) -> Arc<Mutex<SyscallTable>> {
            Arc::clone(&self.syscalls)
        }
    }
}
