pub mod proc {
    use std::io::Error;
    use std::collections::HashMap;
    use std::fs;

    use crate::shared_memory;
    use crate::shared_memory::shared_memory::SharedMemory;

    pub struct Registers {
        pub V: [u8; 16],
        pub DT: u8,
        pub ST: u8,
        pub I: u16,
        pub SP: u16,
        pub PC: u16,
    }

    impl Default for Registers {
        fn default() -> Registers {
            Registers { 
                I: 0,
                V: [0; 16],
                DT: 0,
                ST: 0,
                SP: 0xfa0,
                PC: 0x200
            }
        }
    }

    impl Registers {

        pub fn reg_state(&self) {
            println!("PC - {:x}", self.PC);
            println!("SP - {:x}", self.SP);
            println!("ST - {:x}", self.ST);
            println!("DT - {:x}", self.DT);
            println!("I  - {:x}", self.I);
        
            for i in 0..16 {
                println!("V[{}] - {:x}", i, self.V[i]);
            }
        }
    }

    pub struct ProcessTable<'a> {
        pub procs: HashMap<u32, &'a Proc<'a>>,
    }

    impl<'a> ProcessTable<'a> {
        pub fn new() -> Result<ProcessTable<'a>, Error> {
            Ok(ProcessTable {
                procs: HashMap::new(),
            })
        }
    }

    pub struct Proc<'a> {
        pub proc_id: u32,
        pub regs: Registers,
        pub mem: &'a mut [u8],
    }

    impl<'a> Proc<'a> {
        pub fn new(mem: &'a mut SharedMemory) -> Result<Proc<'a>, Error> {
            let vaddr = mem.mmap(1)?;
            let mem_slice = mem.vaddr_to_pte(vaddr)?;

            Ok(Proc {
                proc_id: 0x41,
                regs: Registers::default(),
                mem: mem_slice,
            })
        }

        pub fn load_program(&mut self, filename: String) -> Result<(), Error> {
            let program_text = fs::read(filename)?;
            if program_text.len() > shared_memory::shared_memory::PAGE_SIZE {
                return Err(Error::new(std::io::ErrorKind::FileTooLarge, "File too large"));
            }

            self.mem[..program_text.len()].copy_from_slice(&program_text);
            Ok(())
        }
    }

}