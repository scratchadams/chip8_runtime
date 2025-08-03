pub mod proc {
    use std::io::Error;
    use std::collections::HashMap;
    use std::fs;

    use crate::shared_memory;
    use crate::shared_memory::shared_memory::SharedMemory;

    const chip8_sprites: [u8; 80] = [
        0xF0, 0x90, 0x90, 0x90, 0xF0, // 0
        0x20, 0x60, 0x20, 0x20, 0x70, // 1
        0xF0, 0x10, 0xF0, 0x80, 0xF0, // 2
        0xF0, 0x10, 0xF0, 0x10, 0xF0, // 3
        0x90, 0x90, 0xF0, 0x10, 0x10, // 4
        0xF0, 0x80, 0xF0, 0x10, 0xF0, // 5
        0xF0, 0x80, 0xF0, 0x90, 0xF0, // 6
        0xF0, 0x10, 0x20, 0x40, 0x40, // 7
        0xF0, 0x90, 0xF0, 0x90, 0xF0, // 8
        0xF0, 0x90, 0xF0, 0x10, 0xF0, // 9
        0xF0, 0x90, 0xF0, 0x90, 0x90, // A
        0xE0, 0x90, 0xE0, 0x90, 0xE0, // B
        0xF0, 0x80, 0x80, 0x80, 0xF0, // C
        0xE0, 0x90, 0x90, 0x90, 0xE0, // D
        0xF0, 0x80, 0xF0, 0x80, 0xF0, // E
        0xF0, 0x80, 0xF0, 0x80, 0x80  // F
    ];

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
            if program_text.len() > shared_memory::shared_memory::PAGE_SIZE - 0x200 {
                return Err(Error::new(std::io::ErrorKind::FileTooLarge, "File too large"));
            }

            //copy sprites into process memory
            self.mem[0x0..0x50].copy_from_slice(&chip8_sprites);

            //copy program text into process memory
            self.mem[0x200..(0x200 + program_text.len())].copy_from_slice(&program_text);
            Ok(())
        }
    }

}