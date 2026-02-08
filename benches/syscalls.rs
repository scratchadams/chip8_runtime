use criterion::{black_box, criterion_group, criterion_main, Criterion};
use chip8_runtime::kernel::kernel::Kernel;
use chip8_runtime::shared_memory::shared_memory::SharedMemory;
use chip8_runtime::display::display::DisplayWindow;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

fn benchmark_syscall_dispatch(c: &mut Criterion) {
    c.bench_function("syscall_dispatch_yield", |b| {
        let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
        let root = PathBuf::from("./roms");
        let mut kernel = Kernel::new(mem, root).unwrap();
        kernel.register_base_syscalls().unwrap();

        let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

        b.iter(|| {
            // Execute SYS_YIELD (0x0104)
            if let Some(entry) = kernel.procs.get_mut(&pid) {
                // Write syscall frame for yield (no args needed)
                let frame = vec![0x01, 0x01, 0x04]; // len=1, syscall_id=0x0104
                entry.proc.write_bytes(0x200, &frame).unwrap();
                entry.proc.regs.I = 0x200;

                // Execute syscall opcode
                entry.proc.execute_opcode(black_box(0x01FF)).unwrap();
            }
        });
    });
}

fn benchmark_syscall_error_logging(c: &mut Criterion) {
    // Benchmark overhead of error logging when disabled
    std::env::remove_var("CHIP8_SYSCALL_ERRORS");

    c.bench_function("syscall_error_logging_disabled", |b| {
        let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
        let root = PathBuf::from("./roms");
        let mut kernel = Kernel::new(mem, root).unwrap();
        kernel.register_base_syscalls().unwrap();

        let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

        b.iter(|| {
            // Execute invalid syscall that will trigger error logging
            if let Some(entry) = kernel.procs.get_mut(&pid) {
                // Write malformed syscall frame (too small)
                entry.proc.write_bytes(0x200, &[0x00]).unwrap();
                entry.proc.regs.I = 0x200;

                // Execute syscall opcode - will fail with ERR_INVALID
                entry.proc.execute_opcode(black_box(0x01FF)).unwrap();
            }
        });
    });
}

fn benchmark_memory_stats_syscall(c: &mut Criterion) {
    c.bench_function("sys_perf_mem_stats", |b| {
        let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
        let root = PathBuf::from("./roms");
        let mut kernel = Kernel::new(mem, root).unwrap();
        kernel.register_base_syscalls().unwrap();

        let pid = kernel.spawn_proc(DisplayWindow::headless(), 1).unwrap();

        b.iter(|| {
            if let Some(entry) = kernel.procs.get_mut(&pid) {
                // Write syscall frame for SYS_PERF_MEM_STATS (0x0140)
                let frame = vec![
                    0x05,       // len=5
                    0x01, 0x40, // syscall_id=0x0140
                    0x02, 0x00, // out_ptr=0x0200
                ];
                entry.proc.write_bytes(0x210, &frame).unwrap();
                entry.proc.regs.I = 0x210;

                // Execute syscall
                entry.proc.execute_opcode(black_box(0x01FF)).unwrap();
            }
        });
    });
}

criterion_group!(
    benches,
    benchmark_syscall_dispatch,
    benchmark_syscall_error_logging,
    benchmark_memory_stats_syscall
);
criterion_main!(benches);
