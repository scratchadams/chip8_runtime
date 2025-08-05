
mod shared_memory;
mod chip8_engine;
mod proc;
mod display;

use shared_memory::shared_memory::SharedMemory;
use proc::proc::{Proc, ProcessTable};
fn main() {
    let mut mem = SharedMemory::new().unwrap();
    let mut read_test: Vec<u8> = Vec::new();
    let mut proc_table = ProcessTable::new().unwrap();

    println!("{:X?}", read_test);

    let mut proc = Proc::new(&mut mem).unwrap();
    proc_table.procs.insert(proc.proc_id, &proc);

    let _ = proc.load_program("/root/rust/chip8/ibm.ch8".to_string());
    proc.run_program();

    proc.regs.reg_state();
    println!("{:X?}", proc.mem);


}
