
mod shared_memory;
mod chip8_engine;
mod proc;
mod display;

use shared_memory::shared_memory::SharedMemory;
use proc::proc::{Proc, ProcessTable};
use std::sync::{Arc, Mutex};
use std::thread;
fn main() {
    let mut mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
    let mut read_test: Vec<u8> = Vec::new();
    let mut proc_table = ProcessTable::new().unwrap();

    println!("{:X?}", read_test);

    let mut mem1 = Arc::clone(&mem);
    let handle1 = thread::spawn(move || {
        let mut proc = Proc::new(&mut mem1).unwrap();

        let _ = proc.load_program("/root/rust/chip8/ibm.ch8".to_string());
        proc.run_program();

        proc.regs.reg_state();
    //let phys = proc.mem.lock().unwrap().phys_mem;
        println!("{:X?}", proc.mem.lock().unwrap().phys_mem);
    });

    let mut mem2 = Arc::clone(&mem);
    let handle2 = thread::spawn(move || {
        let mut proc = Proc::new(&mut mem2).unwrap();

        let _ = proc.load_program("/root/rust/chip8/ibm.ch8".to_string());
        proc.run_program();

        proc.regs.reg_state();
    //let phys = proc.mem.lock().unwrap().phys_mem;
        println!("{:X?}", proc.mem.lock().unwrap().phys_mem);
    });

    //let mut proc2 = Proc::new(&mut mem).unwrap();

    //proc_table.procs.insert(proc.proc_id, proc);
    handle1.join().unwrap();
    handle2.join().unwrap();

    println!("fin..");
}
