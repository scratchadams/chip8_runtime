use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once};
use std::time::{SystemTime, UNIX_EPOCH};

use chip8_runtime::display::display::DisplayWindow;
use chip8_runtime::kernel::kernel::{Kernel, ProcState, SyscallOutcome};
use chip8_runtime::proc::proc::Proc;
use chip8_runtime::shared_memory::shared_memory::SharedMemory;

const MAX_FILENAME_LEN: usize = 64;
const DIR_ENTRY_SIZE: usize = 1 + MAX_FILENAME_LEN + 1 + 4;
const TRACE_RECORD_SIZE: usize = 8;

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

fn write_opcode_at_pc(proc: &mut Proc, opcode: u16) {
    let pc = proc.regs.PC;
    write_opcode(proc, pc, opcode);
}

fn set_input_mode(proc: &mut Proc, mode: u16) {
    write_frame(proc, 0x360, &[mode]);
    proc.regs.I = 0x360;
    write_opcode_at_pc(proc, 0x0112);
}

fn set_console_mode(proc: &mut Proc, mode: u16) {
    write_frame(proc, 0x370, &[mode]);
    proc.regs.I = 0x370;
    write_opcode_at_pc(proc, 0x0113);
}

fn read_dir_entries(proc: &mut Proc, base: u16, count: usize) -> Vec<(String, u8, u32)> {
    let mut entries = Vec::new();
    for idx in 0..count {
        let addr = base as u32 + (idx * DIR_ENTRY_SIZE) as u32;
        let name_len = proc.read_u8(addr).unwrap() as usize;
        let name_bytes = proc.read_bytes(addr + 1, name_len).unwrap();
        let name = String::from_utf8_lossy(&name_bytes).to_string();
        let kind = proc.read_u8(addr + 1 + MAX_FILENAME_LEN as u32).unwrap();
        let size_bytes = proc.read_bytes(addr + 1 + MAX_FILENAME_LEN as u32 + 1, 4).unwrap();
        let size = u32::from_be_bytes([size_bytes[0], size_bytes[1], size_bytes[2], size_bytes[3]]);
        entries.push((name, kind, size));
    }
    entries
}

fn read_u16_be(buf: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([buf[offset], buf[offset + 1]])
}

fn read_trace_records(buf: &[u8], count: usize) -> Vec<(u8, u8, u16, u16, u16)> {
    let mut records = Vec::new();
    for idx in 0..count {
        let base = idx * TRACE_RECORD_SIZE;
        let kind = buf[base];
        let op = buf[base + 1];
        let pid = read_u16_be(buf, base + 2);
        let arg0 = read_u16_be(buf, base + 4);
        let arg1 = read_u16_be(buf, base + 6);
        records.push((kind, op, pid, arg0, arg1));
    }
    records
}

#[test]
fn sys_write_sets_v0_and_vf() {
    set_headless();
    let root = temp_root("write");
    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        proc.write_bytes(0x320, &[0x00]).unwrap();
        write_frame(&mut proc, 0x300, &[0x0320, 1]);
        proc.regs.I = 0x300;
        write_opcode(&mut proc, 0x200, 0x0110);
    }

    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    let proc = kernel.proc(pid).unwrap();
    assert_eq!(proc.regs.V[0], 1);
    assert_eq!(proc.regs.V[0xF], 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_dbg_regs_reads_target() {
    set_headless();
    let root = temp_root("dbg_regs");
    let mut kernel = make_kernel(&root);

    let target_pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();
    let debug_pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let mut target = kernel.proc_mut(target_pid).unwrap();
        target.regs.PC = 0x234;
        target.regs.SP = 0x345;
        target.regs.I = 0x456;
        target.regs.V[0] = 0xAA;
        target.regs.V[1] = 0xBB;
        target.regs.DT = 0x11;
        target.regs.ST = 0x22;
    }

    {
        let mut debug = kernel.proc_mut(debug_pid).unwrap();
        write_frame(&mut debug, 0x300, &[target_pid as u16, 0x0340]);
        debug.regs.I = 0x300;
        write_opcode_at_pc(&mut debug, 0x0131);
    }

    let outcome = kernel.step_proc(debug_pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    let mut debug = kernel.proc_mut(debug_pid).unwrap();
    let data = debug.read_bytes(0x0340, 24).unwrap();
    assert_eq!(read_u16_be(&data, 0), 0x234);
    assert_eq!(read_u16_be(&data, 2), 0x345);
    assert_eq!(read_u16_be(&data, 4), 0x456);
    assert_eq!(data[6], 0xAA);
    assert_eq!(data[7], 0xBB);
    assert_eq!(data[22], 0x11);
    assert_eq!(data[23], 0x22);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_dbg_trace_read_reports_syscall() {
    set_headless();
    let root = temp_root("dbg_trace");
    let mut kernel = make_kernel(&root);

    let target_pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();
    let debug_pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let mut target = kernel.proc_mut(target_pid).unwrap();
        write_opcode_at_pc(&mut target, 0x0104); // sys_yield
    }
    let _ = kernel.step_proc(target_pid).unwrap();

    {
        let mut debug = kernel.proc_mut(debug_pid).unwrap();
        write_frame(&mut debug, 0x300, &[0x0340, 8]);
        debug.regs.I = 0x300;
        write_opcode_at_pc(&mut debug, 0x0134);
    }
    let outcome = kernel.step_proc(debug_pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    let mut debug = kernel.proc_mut(debug_pid).unwrap();
    let count = debug.regs.V[0] as usize;
    assert!(count > 0);
    let data = debug.read_bytes(0x0340, count * TRACE_RECORD_SIZE).unwrap();
    let records = read_trace_records(&data, count);

    let saw_yield = records.iter().any(|(kind, op, _pid, arg0, _)| {
        *kind == 0x02 && *op == 0x02 && *arg0 == 0x0104
    });
    assert!(saw_yield);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_dbg_trace_orders_spawn_before_syscall() {
    set_headless();
    let root = temp_root("dbg_trace_order");
    let mut kernel = make_kernel(&root);

    let target_pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();
    let debug_pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let mut target = kernel.proc_mut(target_pid).unwrap();
        write_opcode_at_pc(&mut target, 0x0104); // sys_yield
    }
    let _ = kernel.step_proc(target_pid).unwrap();

    {
        let mut debug = kernel.proc_mut(debug_pid).unwrap();
        write_frame(&mut debug, 0x300, &[0x0340, 16]);
        debug.regs.I = 0x300;
        write_opcode_at_pc(&mut debug, 0x0134);
    }
    let outcome = kernel.step_proc(debug_pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    let mut debug = kernel.proc_mut(debug_pid).unwrap();
    let count = debug.regs.V[0] as usize;
    assert!(count > 0);
    let data = debug.read_bytes(0x0340, count * TRACE_RECORD_SIZE).unwrap();
    let records = read_trace_records(&data, count);

    let target = target_pid as u16;
    let spawn_idx = records
        .iter()
        .position(|(kind, op, pid, _, _)| *kind == 0x01 && *op == 0x01 && *pid == target)
        .expect("spawn record missing");
    let syscall_idx = records
        .iter()
        .position(|(kind, op, pid, arg0, _)| {
            *kind == 0x02 && *op == 0x02 && *pid == target && *arg0 == 0x0104
        })
        .expect("syscall record missing");

    assert!(spawn_idx < syscall_idx);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_read_copies_input() {
    set_headless();
    let root = temp_root("read");
    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        set_input_mode(&mut proc, 1);
    }
    let _ = kernel.step_proc(pid).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        write_frame(&mut proc, 0x300, &[0x0340, 2]);
        proc.regs.I = 0x300;
        write_opcode_at_pc(&mut proc, 0x0111);
    }

    kernel.push_input(b"ok");
    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    let mut proc = kernel.proc_mut(pid).unwrap();
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
        let mut proc = kernel.proc_mut(pid).unwrap();
        write_frame(&mut proc, 0x300, &[0x0340, 4]);
        proc.regs.I = 0x300;
        write_opcode_at_pc(&mut proc, 0x0111);
    }

    kernel.push_input(b"hi");
    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Blocked);
    assert_eq!(kernel.proc_state(pid), Some(ProcState::Blocked));

    kernel.push_input(b"\n");
    assert_eq!(kernel.proc_state(pid), Some(ProcState::Running));
    let mut proc = kernel.proc_mut(pid).unwrap();
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
        let mut proc = kernel.proc_mut(pid).unwrap();
        set_input_mode(&mut proc, 1);
    }
    let _ = kernel.step_proc(pid).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        write_frame(&mut proc, 0x300, &[0x2000, 1]);
        proc.regs.I = 0x300;
        write_opcode_at_pc(&mut proc, 0x0111);
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
fn sys_read_console_uses_console_input() {
    set_headless();
    let root = temp_root("read_console");
    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        set_console_mode(&mut proc, 1);
    }
    let _ = kernel.step_proc(pid).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        set_input_mode(&mut proc, 1);
    }
    let _ = kernel.step_proc(pid).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        write_frame(&mut proc, 0x300, &[0x0340, 2]);
        proc.regs.I = 0x300;
        write_opcode_at_pc(&mut proc, 0x0111);
    }

    kernel.push_console_input(pid, b"ok");
    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    let mut proc = kernel.proc_mut(pid).unwrap();
    let data = proc.read_bytes(0x340, 2).unwrap();
    assert_eq!(data, b"ok");
    assert_eq!(proc.regs.V[0], 2);
    assert_eq!(proc.regs.V[0xF], 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_write_console_updates_display() {
    set_headless();
    let root = temp_root("write_console");
    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        set_console_mode(&mut proc, 1);
    }
    let _ = kernel.step_proc(pid).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        proc.write_bytes(0x320, b"a").unwrap();
        write_frame(&mut proc, 0x300, &[0x0320, 1]);
        proc.regs.I = 0x300;
        write_opcode_at_pc(&mut proc, 0x0110);
    }

    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    let proc = kernel.proc(pid).unwrap();
    assert!(proc.display.buf.iter().any(|&px| px != 0));

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
        let mut proc = kernel.proc_mut(pid_waiter).unwrap();
        write_frame(&mut proc, 0x300, &[pid_target as u16]);
        proc.regs.I = 0x300;
        write_opcode(&mut proc, 0x200, 0x0103);
    }

    let outcome = kernel.step_proc(pid_waiter).unwrap();
    assert_eq!(outcome, SyscallOutcome::Blocked);
    assert_eq!(kernel.proc_state(pid_waiter), Some(ProcState::Blocked));

    {
        let mut proc = kernel.proc_mut(pid_target).unwrap();
        write_frame(&mut proc, 0x320, &[0x002A]);
        proc.regs.I = 0x320;
        write_opcode(&mut proc, 0x200, 0x0102);
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
        let mut proc = kernel.proc_mut(pid).unwrap();
        proc.write_bytes(0x340, b"child.ch8").unwrap();
        write_frame(&mut proc, 0x300, &[0x0340, 9, 1]);
        proc.regs.I = 0x300;
        write_opcode(&mut proc, 0x200, 0x0101);
    }

    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    let proc = kernel.proc(pid).unwrap();
    assert_eq!(proc.regs.V[0xF], 0);
    let child_pid = proc.regs.V[0] as u32;
    assert!(kernel.proc(child_pid).is_some());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_fs_list_reads_root_entries() {
    set_headless();
    let root = temp_root("fs_list");
    fs::write(root.join("a.ch8"), vec![1, 2, 3]).unwrap();
    fs::create_dir(root.join("subdir")).unwrap();

    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        write_frame(&mut proc, 0x300, &[0x0000, 0, 0x0400, 10]);
        proc.regs.I = 0x300;
        write_opcode_at_pc(&mut proc, 0x0120);
    }

    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    let mut proc = kernel.proc_mut(pid).unwrap();
    assert_eq!(proc.regs.V[0xF], 0);
    let count = proc.regs.V[0] as usize;
    let entries = read_dir_entries(&mut proc, 0x0400, count);
    assert!(entries.iter().any(|(n, k, _)| n == "a.ch8" && *k == 0));
    assert!(entries.iter().any(|(n, k, _)| n == "subdir" && *k == 1));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_fs_open_read_close_roundtrip() {
    set_headless();
    let root = temp_root("fs_open");
    fs::write(root.join("hello.txt"), b"hello").unwrap();

    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        proc.write_bytes(0x340, b"hello.txt").unwrap();
        write_frame(&mut proc, 0x300, &[0x0340, 9, 0]);
        proc.regs.I = 0x300;
        write_opcode_at_pc(&mut proc, 0x0121);
    }

    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);
    let fd = kernel.proc(pid).unwrap().regs.V[0];
    assert_ne!(fd, 0);

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        write_frame(&mut proc, 0x320, &[fd as u16, 0x0500, 5]);
        proc.regs.I = 0x320;
        write_opcode_at_pc(&mut proc, 0x0122);
    }

    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);
    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        let data = proc.read_bytes(0x0500, 5).unwrap();
        assert_eq!(data, b"hello");
        assert_eq!(proc.regs.V[0], 5);
    }

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        write_frame(&mut proc, 0x340, &[fd as u16]);
        proc.regs.I = 0x340;
        write_opcode_at_pc(&mut proc, 0x0123);
    }
    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    let _ = fs::remove_dir_all(root);
}

// Negative tests for edge cases

#[test]
fn sys_spawn_rejects_frame_too_small() {
    set_headless();
    let root = temp_root("malformed_frame_length");
    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        // Write frame with length too small to contain required args.
        // sys_spawn needs 3 args (name_ptr, name_len, pages) = 1 + 3*2 = 7 bytes minimum
        proc.regs.I = 0x300;
        proc.write_u8(0x300, 3).unwrap();  // Length 3, but needs at least 7
        write_opcode_at_pc(&mut proc, 0x0101);  // sys_spawn
    }

    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    {
        let proc = kernel.proc(pid).unwrap();
        // Should fail with invalid argument error (frame too small)
        assert_eq!(proc.regs.V[0xF], 1, "VF should be 1 (error)");
        assert_eq!(proc.regs.V[0], 0x02, "V0 should be 0x02 (invalid argument)");
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_write_rejects_buffer_outside_memory() {
    set_headless();
    let root = temp_root("invalid_buffer_address");
    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        // Write frame pointing to address beyond vm_size (4096 for 1-page proc)
        write_frame(&mut proc, 0x300, &[0xF000, 100]);  // Address 0xF000 is out of bounds
        proc.regs.I = 0x300;
        write_opcode_at_pc(&mut proc, 0x0110);  // sys_write
    }

    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    {
        let proc = kernel.proc(pid).unwrap();
        // Should fail with invalid argument error
        assert_eq!(proc.regs.V[0xF], 1, "VF should be 1 (error)");
        assert_eq!(proc.regs.V[0], 0x02, "V0 should be 0x02 (invalid argument)");
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_set_timing_configures_process_timing() {
    set_headless();
    let root = temp_root("set_timing");
    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        // SYS_SET_TIMING frame:
        //   target_pid=0 (self), timeslice_steps=1000, target_ips=3600, timer_hz=60
        write_frame(&mut proc, 0x300, &[
            0,     // target_pid (0 = self)
            1000,  // timeslice_steps
            3600,  // target_ips (60Hz legacy mode)
            60,    // timer_hz
        ]);
        proc.regs.I = 0x300;
        write_opcode_at_pc(&mut proc, 0x0142);  // SYS_SET_TIMING
    }

    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    {
        let proc = kernel.proc(pid).unwrap();
        // Should succeed
        assert_eq!(proc.regs.V[0xF], 0, "VF should be 0 (success)");
        assert_eq!(proc.regs.V[0], 1, "V0 should be 1 (success)");
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sys_set_timing_rejects_invalid_process() {
    set_headless();
    let root = temp_root("set_timing_invalid_pid");
    let mut kernel = make_kernel(&root);
    let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

    {
        let mut proc = kernel.proc_mut(pid).unwrap();
        // Try to configure non-existent process (PID 9999)
        write_frame(&mut proc, 0x300, &[9999, 1000, 0, 0]);
        proc.regs.I = 0x300;
        write_opcode_at_pc(&mut proc, 0x0142);  // SYS_SET_TIMING
    }

    let outcome = kernel.step_proc(pid).unwrap();
    assert_eq!(outcome, SyscallOutcome::Completed);

    {
        let proc = kernel.proc(pid).unwrap();
        // Should fail with not found error
        assert_eq!(proc.regs.V[0xF], 1, "VF should be 1 (error)");
        assert_eq!(proc.regs.V[0], 0x04, "V0 should be 0x04 (not found)");
    }

    let _ = fs::remove_dir_all(root);
}
