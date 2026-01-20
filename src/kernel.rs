pub mod kernel {
    use std::collections::HashMap;
    use std::io::{Error, ErrorKind};
    use std::sync::{Arc, Mutex};

    use crate::display::display::DisplayWindow;
    use crate::proc::proc::Proc;
    use crate::shared_memory::shared_memory::SharedMemory;

    // Codex generated: syscall handlers return whether the caller should advance PC.
    pub enum SyscallOutcome {
        Completed,
        Blocked,
    }

    pub type SyscallHandler = fn(&mut Proc) -> SyscallOutcome;

    pub struct SyscallTable {
        handlers: HashMap<u16, SyscallHandler>,
    }

    impl SyscallTable {
        pub fn new() -> SyscallTable {
            SyscallTable {
                handlers: HashMap::new(),
            }
        }

        pub fn register(&mut self, id: u16, handler: SyscallHandler) -> Result<(), Error> {
            if !(0x0100..0x0200).contains(&id) {
                return Err(Error::new(ErrorKind::InvalidInput, "syscall id out of range"));
            }
            self.handlers.insert(id, handler);
            Ok(())
        }

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
        pub fn new(mem: Arc<Mutex<SharedMemory>>) -> Kernel {
            Kernel {
                mem,
                syscalls: Arc::new(Mutex::new(SyscallTable::new())),
                procs: HashMap::new(),
                next_pid: 1,
            }
        }

        pub fn register_syscall(&mut self, id: u16, handler: SyscallHandler) -> Result<(), Error> {
            self.syscalls
                .lock()
                .unwrap()
                .register(id, handler)
        }

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

        pub fn proc_mut(&mut self, pid: u32) -> Option<&mut Proc> {
            self.procs.get_mut(&pid)
        }

        pub fn syscall_table(&self) -> Arc<Mutex<SyscallTable>> {
            Arc::clone(&self.syscalls)
        }
    }
}
