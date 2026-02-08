use criterion::{black_box, criterion_group, criterion_main, Criterion};
use chip8_runtime::shared_memory::shared_memory::SharedMemory;
use chip8_runtime::display::display::DisplayWindow;
use chip8_core::chip8_engine::chip8_engine::Proc;
use std::sync::{Arc, Mutex};

fn benchmark_arithmetic_opcodes(c: &mut Criterion) {
    c.bench_function("opcode_8xy4_add", |b| {
        let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
        let mut proc = Proc::new_with_display_and_pages(mem, DisplayWindow::headless(), 1).unwrap();

        proc.regs.V[1] = 0x10;
        proc.regs.V[2] = 0x20;

        b.iter(|| {
            proc.execute_opcode(black_box(0x8124)).unwrap(); // ADD V1, V2
        });
    });

    c.bench_function("opcode_8xy5_sub", |b| {
        let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
        let mut proc = Proc::new_with_display_and_pages(mem, DisplayWindow::headless(), 1).unwrap();

        proc.regs.V[1] = 0x30;
        proc.regs.V[2] = 0x10;

        b.iter(|| {
            proc.execute_opcode(black_box(0x8125)).unwrap(); // SUB V1, V2
        });
    });
}

fn benchmark_memory_opcodes(c: &mut Criterion) {
    c.bench_function("opcode_fx55_store_regs", |b| {
        let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
        let mut proc = Proc::new_with_display_and_pages(mem, DisplayWindow::headless(), 1).unwrap();

        proc.regs.I = 0x300;
        for i in 0..16 {
            proc.regs.V[i] = i as u8;
        }

        b.iter(|| {
            proc.execute_opcode(black_box(0xFF55)).unwrap(); // LD [I], VF
        });
    });

    c.bench_function("opcode_fx65_load_regs", |b| {
        let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
        let mut proc = Proc::new_with_display_and_pages(mem, DisplayWindow::headless(), 1).unwrap();

        proc.regs.I = 0x300;
        // Pre-fill memory
        for i in 0..16 {
            proc.write_bytes(0x300 + i, &[i as u8]).unwrap();
        }

        b.iter(|| {
            proc.execute_opcode(black_box(0xFF65)).unwrap(); // LD VF, [I]
        });
    });
}

fn benchmark_control_flow_opcodes(c: &mut Criterion) {
    c.bench_function("opcode_2nnn_call", |b| {
        let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
        let mut proc = Proc::new_with_display_and_pages(mem, DisplayWindow::headless(), 1).unwrap();

        proc.regs.PC = 0x200;
        proc.regs.SP = proc.vm_size as u16;

        b.iter(|| {
            proc.execute_opcode(black_box(0x2345)).unwrap(); // CALL 0x345
            // Reset PC and SP for next iteration
            proc.regs.PC = 0x200;
            proc.regs.SP = proc.vm_size as u16;
        });
    });

    c.bench_function("opcode_00ee_ret", |b| {
        let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
        let mut proc = Proc::new_with_display_and_pages(mem, DisplayWindow::headless(), 1).unwrap();

        // Set up stack with return address
        proc.regs.SP = proc.vm_size as u16 - 2;
        proc.write_bytes(proc.regs.SP as u32, &[0x02, 0x00]).unwrap();

        b.iter(|| {
            proc.execute_opcode(black_box(0x00EE)).unwrap(); // RET
            // Reset SP for next iteration
            proc.regs.SP = proc.vm_size as u16 - 2;
        });
    });
}

fn benchmark_logical_opcodes(c: &mut Criterion) {
    c.bench_function("opcode_8xy2_and", |b| {
        let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
        let mut proc = Proc::new_with_display_and_pages(mem, DisplayWindow::headless(), 1).unwrap();

        proc.regs.V[1] = 0xF0;
        proc.regs.V[2] = 0x0F;

        b.iter(|| {
            proc.execute_opcode(black_box(0x8122)).unwrap(); // AND V1, V2
        });
    });

    c.bench_function("opcode_8xy1_or", |b| {
        let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
        let mut proc = Proc::new_with_display_and_pages(mem, DisplayWindow::headless(), 1).unwrap();

        proc.regs.V[1] = 0xF0;
        proc.regs.V[2] = 0x0F;

        b.iter(|| {
            proc.execute_opcode(black_box(0x8121)).unwrap(); // OR V1, V2
        });
    });
}

criterion_group!(
    benches,
    benchmark_arithmetic_opcodes,
    benchmark_memory_opcodes,
    benchmark_control_flow_opcodes,
    benchmark_logical_opcodes
);
criterion_main!(benches);
