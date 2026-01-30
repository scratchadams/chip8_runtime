use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once};
use std::time::{SystemTime, UNIX_EPOCH};

use chip8_runtime::display::display::DisplayWindow;
use chip8_runtime::kernel::kernel::{
    Kernel, ScheduleOutcome, ScheduleReason, SchedulerEvent, SchedulerPolicy,
};
use chip8_runtime::proc::proc::Proc;
use chip8_runtime::shared_memory::shared_memory::SharedMemory;

static INIT: Once = Once::new();

fn set_headless() {
    INIT.call_once(|| {
        // set_var is unsafe on this toolchain; tests run single-process here.
        unsafe {
            std::env::set_var("CHIP8_HEADLESS", "1");
        }
    });
}

fn temp_root(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut path = std::env::temp_dir();
    path.push(format!("chip8_runtime_{label}_{nanos}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn make_kernel(root: &Path) -> Kernel {
    let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
    let mut kernel = Kernel::new(mem, root.to_path_buf()).unwrap();
    kernel.register_base_syscalls().unwrap();
    kernel
}

fn write_opcode(proc: &mut Proc, addr: u16, opcode: u16) {
    let hi = (opcode >> 8) as u8;
    let lo = opcode as u8;
    proc.write_bytes(addr as u32, &[hi, lo]).unwrap();
}

struct TestScheduler {
    queue: VecDeque<u32>,
    events: Arc<Mutex<Vec<(SchedulerEvent, u32)>>>,
}

impl TestScheduler {
    fn new(events: Arc<Mutex<Vec<(SchedulerEvent, u32)>>>) -> Self {
        Self {
            queue: VecDeque::new(),
            events,
        }
    }
}

impl SchedulerPolicy for TestScheduler {
    fn on_runnable(&mut self, pid: u32, event: SchedulerEvent) {
        self.events.lock().unwrap().push((event, pid));
        if !self.queue.contains(&pid) {
            self.queue.push_back(pid);
        }
    }

    fn on_blocked(&mut self, pid: u32) {
        self.queue.retain(|&entry| entry != pid);
    }

    fn on_exit(&mut self, pid: u32) {
        self.queue.retain(|&entry| entry != pid);
    }

    fn next(&mut self) -> Option<u32> {
        self.queue.pop_front()
    }

    fn has_runnable(&self) -> bool {
        !self.queue.is_empty()
    }
}

#[test]
fn preemption_round_robin_interleaves() {
    set_headless();
    let root = temp_root("preempt_rr");
    let mut kernel = make_kernel(&root);
    kernel.set_timeslice_steps(2);

    let pid_a = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();
    let pid_b = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    for pid in [pid_a, pid_b] {
        let proc = kernel.proc_mut(pid).unwrap();
        proc.regs.PC = 0x200;
        write_opcode(proc, 0x200, 0x7001); // add 1 to V0
        write_opcode(proc, 0x202, 0x1200); // jump back to 0x200
    }

    let out1 = kernel.schedule_once().unwrap();
    let pid1 = match out1 {
        ScheduleOutcome::Ran { pid, reason } => {
            assert_eq!(reason, ScheduleReason::Preempted);
            pid
        }
        ScheduleOutcome::Idle => panic!("expected a runnable pid"),
    };
    assert_eq!(kernel.proc(pid1).unwrap().regs.V[0], 1);

    let out2 = kernel.schedule_once().unwrap();
    let pid2 = match out2 {
        ScheduleOutcome::Ran { pid, reason } => {
            assert_eq!(reason, ScheduleReason::Preempted);
            pid
        }
        ScheduleOutcome::Idle => panic!("expected a runnable pid"),
    };

    assert_ne!(pid1, pid2);
    assert_eq!(kernel.proc(pid2).unwrap().regs.V[0], 1);

    let out3 = kernel.schedule_once().unwrap();
    let pid3 = match out3 {
        ScheduleOutcome::Ran { pid, reason } => {
            assert_eq!(reason, ScheduleReason::Preempted);
            pid
        }
        ScheduleOutcome::Idle => panic!("expected a runnable pid"),
    };

    assert_eq!(pid3, pid1);
    assert_eq!(kernel.proc(pid1).unwrap().regs.V[0], 2);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn scheduler_policy_hooks_record_yield() {
    set_headless();
    let root = temp_root("policy_hooks");
    let mut kernel = make_kernel(&root);

    let events = Arc::new(Mutex::new(Vec::new()));
    kernel.set_scheduler_policy(Box::new(TestScheduler::new(Arc::clone(&events))));

    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();
    {
        let proc = kernel.proc_mut(pid).unwrap();
        proc.regs.PC = 0x200;
        write_opcode(proc, 0x200, 0x0104); // sys_yield
    }

    let out = kernel.schedule_once().unwrap();
    assert_eq!(out, ScheduleOutcome::Ran { pid, reason: ScheduleReason::Yielded });

    let events = events.lock().unwrap().clone();
    assert!(events.contains(&(SchedulerEvent::Spawned, pid)));
    assert!(events.contains(&(SchedulerEvent::Yielded, pid)));

    let _ = fs::remove_dir_all(root);
}
