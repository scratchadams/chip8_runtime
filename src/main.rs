
mod shared_memory;

use shared_memory::shared_memory::SharedMemory;
fn main() {
    let mut mem = SharedMemory::new().unwrap();
    let mut read_test: Vec<u8> = Vec::new();

    /*for i in 0..56 {
        let _ = mem.mmap(1);
    }*/

    let _ = mem.mmap(1);

    //let _ = mem.write(0x12111, vec![0x1, 0x5, 0x41, 0x12]);
    let _ = mem.load_program(0x0000, "/root/machineid".to_string());

    let _ = mem.read(0x0000,&mut read_test, 0x24);

    println!("{:X?}", read_test);


}
