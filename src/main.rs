
mod shared_memory;
mod chip8_engine;
mod proc;
mod display;

use shared_memory::shared_memory::SharedMemory;
use proc::proc::{Proc, ProcessTable};
use std::sync::{Arc, Mutex};
use std::thread;
fn main() {
    //Allocate system memory
    let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));

    // Codex generated: placeholder debug output; read_test is always empty right now.
    let read_test: Vec<u8> = Vec::new();
    println!("{:X?}", read_test);

    // Codex generated: mem1 is an Arc clone of mem, so both threads share the same allocation.
    // Codex generated: the Mutex enforces synchronized access to the shared memory contents.
    // Codex generated: each thread owns its own Arc handle; the data stays shared.
    let mut mem1 = Arc::clone(&mem);
    let handle1 = thread::spawn(move || {
        let mut proc = Proc::new(&mut mem1).unwrap();

        let _ = proc.load_program("/root/rust/chip8/ibm.ch8".to_string());
        proc.run_program();

        proc.regs.reg_state();
        println!("{:X?}", proc.mem.lock().unwrap().phys_mem);
    });

    let mut mem2 = Arc::clone(&mem);
    let handle2 = thread::spawn(move || {
        // Codex generated: each Proc owns its DisplayWindow; only memory is shared.
        let mut proc = Proc::new(&mut mem2).unwrap();

        let _ = proc.load_program("/root/rust/chip8/br8kout.ch8".to_string());
        proc.run_program();

        proc.regs.reg_state();
        println!("{:X?}", proc.mem.lock().unwrap().phys_mem);
    });

    handle1.join().unwrap();
    handle2.join().unwrap();

    println!("fin..");
}
