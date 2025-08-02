
mod shared_memory;
mod chip8_engine;
mod proc;

use shared_memory::shared_memory::SharedMemory;
use proc::proc::Proc;
use crate::proc::proc::ProcessTable;
fn main() {
    let mut mem = SharedMemory::new().unwrap();
    let mut read_test: Vec<u8> = Vec::new();
    let mut proc_table = ProcessTable::new().unwrap();
    //let _ = mem.mmap(1);

    //let _ = mem.write(0x12111, vec![0x1, 0x5, 0x41, 0x12]);
    //let _ = mem.load_program(0x0000, "/root/machineid".to_string());

    //let _ = mem.read(0x0000,&mut read_test, 0x24);

    println!("{:X?}", read_test);

    let mut proc = Proc::new(&mut mem).unwrap();
    proc_table.procs.insert(proc.proc_id, &proc);

    let _ = proc.load_program("/root/machineid".to_string());

    proc.regs.reg_state();
    println!("{:X?}", proc.mem);


}
