
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

    //Not sure what this test was for, will keep for now
    let read_test: Vec<u8> = Vec::new();
    println!("{:X?}", read_test);

    //mem1 is an Arc clone of mem, which I believe is used as a reference
    //to system memory. That way we pass the Arc clone to our new process
    //so it can safely use that memory TODO: re-read the Arc docs and tutorial.
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
