use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once};
use std::time::{SystemTime, UNIX_EPOCH};

use chip8_runtime::display::display::DisplayWindow;
use chip8_runtime::kernel::kernel::{Kernel, ProcState, SyscallOutcome};
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

fn write_frame(proc: &mut Proc, base: u16, args: &[u16]) {
    let len = 1 + args.len() * 2;
    let mut data = Vec::with_capacity(len);
    data.push(len as u8);
    for arg in args {
        data.push((arg >> 8) as u8);
        data.push(*arg as u8);
    }
    proc.write_bytes(base as u32, &data).unwrap();
}

fn set_input_mode(proc: &mut Proc, mode: u16) {
    write_frame(proc, 0x360, &[mode]);
    proc.regs.I = 0x360;
    write_opcode(proc, proc.regs.PC, 0x0112);
}

#[test]
fn sys_write_sets_v0_and_vf() {
    set_headless();
    let root = temp_root("write");
    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let proc = kernel.proc_mut(pid).unwrap();
        proc.write_bytes(0x320, &[0x00]).unwrap();
        write_frame(proc, 0x300, &[0x0320, 1]);
        proc.regs.I = 0x300;
        write_opcode(proc, 0x200, 0x0110);
    }

    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    let proc = kernel.proc(pid).unwrap();
    assert_eq!(proc.regs.V[0], 1);
    assert_eq!(proc.regs.V[0xF], 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_read_copies_input() {
    set_headless();
    let root = temp_root("read");
    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let proc = kernel.proc_mut(pid).unwrap();
        set_input_mode(proc, 1);
    }
    let _ = kernel.step_proc(pid).unwrap();

    {
        let proc = kernel.proc_mut(pid).unwrap();
        write_frame(proc, 0x300, &[0x0340, 2]);
        proc.regs.I = 0x300;
        write_opcode(proc, proc.regs.PC, 0x0111);
    }

    kernel.push_input(b"ok");
    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    let proc = kernel.proc_mut(pid).unwrap();
    let data = proc.read_bytes(0x340, 2).unwrap();
    assert_eq!(data, b"ok");
    assert_eq!(proc.regs.V[0], 2);
    assert_eq!(proc.regs.V[0xF], 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_read_line_blocks_until_newline() {
    set_headless();
    let root = temp_root("read_line");
    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let proc = kernel.proc_mut(pid).unwrap();
        write_frame(proc, 0x300, &[0x0340, 4]);
        proc.regs.I = 0x300;
        write_opcode(proc, proc.regs.PC, 0x0111);
    }

    kernel.push_input(b"hi");
    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Blocked);
    assert_eq!(kernel.proc_state(pid), Some(ProcState::Blocked));

    kernel.push_input(b"\n");
    assert_eq!(kernel.proc_state(pid), Some(ProcState::Running));
    let proc = kernel.proc_mut(pid).unwrap();
    let data = proc.read_bytes(0x340, 3).unwrap();
    assert_eq!(data, b"hi\n");
    assert_eq!(proc.regs.V[0], 3);
    assert_eq!(proc.regs.V[0xF], 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_read_rejects_invalid_buffer() {
    set_headless();
    let root = temp_root("read_invalid");
    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let proc = kernel.proc_mut(pid).unwrap();
        set_input_mode(proc, 1);
    }
    let _ = kernel.step_proc(pid).unwrap();

    {
        let proc = kernel.proc_mut(pid).unwrap();
        write_frame(proc, 0x300, &[0x2000, 1]);
        proc.regs.I = 0x300;
        write_opcode(proc, proc.regs.PC, 0x0111);
    }

    kernel.push_input(b"z");
    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);
    let proc = kernel.proc(pid).unwrap();
    assert_eq!(proc.regs.V[0], 0x02);
    assert_eq!(proc.regs.V[0xF], 1);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_wait_unblocks_on_exit() {
    set_headless();
    let root = temp_root("wait");
    let mut kernel = make_kernel(&root);

    let pid_target = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();
    let pid_waiter = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let proc = kernel.proc_mut(pid_waiter).unwrap();
        write_frame(proc, 0x300, &[pid_target as u16]);
        proc.regs.I = 0x300;
        write_opcode(proc, 0x200, 0x0103);
    }

    let outcome = kernel.step_proc(pid_waiter).unwrap();
    assert_eq!(outcome, SyscallOutcome::Blocked);
    assert_eq!(kernel.proc_state(pid_waiter), Some(ProcState::Blocked));

    {
        let proc = kernel.proc_mut(pid_target).unwrap();
        write_frame(proc, 0x320, &[0x002A]);
        proc.regs.I = 0x320;
        write_opcode(proc, 0x200, 0x0102);
    }

    let _ = kernel.step_proc(pid_target).unwrap();
    assert_eq!(kernel.proc_state(pid_waiter), Some(ProcState::Running));

    let proc = kernel.proc(pid_waiter).unwrap();
    assert_eq!(proc.regs.V[0], 0x2A);
    assert_eq!(proc.regs.V[0xF], 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_spawn_creates_process() {
    set_headless();
    let root = temp_root("spawn");
    let rom_path = root.join("child.ch8");
    fs::write(&rom_path, vec![0x01, 0x02]).unwrap();

    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let proc = kernel.proc_mut(pid).unwrap();
        proc.write_bytes(0x340, b"child.ch8").unwrap();
        write_frame(proc, 0x300, &[0x0340, 9, 1]);
        proc.regs.I = 0x300;
        write_opcode(proc, 0x200, 0x0101);
    }

    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    let proc = kernel.proc(pid).unwrap();
    assert_eq!(proc.regs.V[0xF], 0);
    let child_pid = proc.regs.V[0] as u32;
    assert!(kernel.proc(child_pid).is_some());

    let _ = fs::remove_dir_all(root);
}
