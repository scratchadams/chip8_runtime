pub mod proc {
    use std::io::Error;
    use std::collections::HashMap;
    use std::fs;

    use crate::chip8_engine::chip8_engine::*;
    use crate::shared_memory;
    use crate::shared_memory::shared_memory::SharedMemory;
    use crate::display::display::DisplayWindow;

    macro_rules! extract_opcode {
        ($value:expr) => {
            ($value >> 0xc) as u8
        };
    }

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
        pub display: DisplayWindow,
    }

    impl<'a> Proc<'a> {
        pub fn new(mem: &'a mut SharedMemory) -> Result<Proc<'a>, Error> {
            let vaddr = mem.mmap(1)?;
            let mem_slice = mem.vaddr_to_pte(vaddr)?;

            let display = DisplayWindow::new().unwrap();

            Ok(Proc {
                proc_id: 0x41,
                regs: Registers::default(),
                mem: mem_slice,
                display: display,
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

        pub fn run_program(&mut self) {
            loop {
                let pc = self.regs.PC as usize;
                let mut instruction = ((self.mem[pc] as u16) << 8) | self.mem[pc+1] as u16;
                let opcode = extract_opcode!(instruction);

                match opcode {
                    0x0 => {
                        opcode_0x0(self, instruction);
                    },
                    0x1 => {
                        opcode_0x1(self, instruction);
                    },
                    0x2 => {
                        opcode_0x2(self, instruction);
                    },
                    0x3 => {
                        opcode_0x3(self, instruction);
                    },
                    0x4 => {
                        opcode_0x4(self, instruction);
                    },
                    0x5 => {
                        opcode_0x5(self, instruction);
                    },
                    0x6 => {
                        opcode_0x6(self, instruction);
                    },
                    0x7 => {
                        opcode_0x7(self, instruction);
                    },
                    0x8 => {
                        opcode_0x8(self, instruction);
                    },
                    0x9 => {
                        opcode_0x9(self, instruction);
                    },
                    0xA => {
                        opcode_0xa(self, instruction);
                    },
                    0xB => {
                        opcode_0xb(self, instruction);
                    },
                    0xC => {
                        opcode_0xc(self, instruction);
                    },
                    0xD => {
                        opcode_0xd(self, instruction);
                    },
                    0xE => {
                        opcode_0xe(self, instruction);
                    },
                    0xF => {
                        opcode_0xf(self, instruction);
                    },
                    _ => {
                        panic!("Unknown opcode: {:X}", opcode);
                    }
                }
            }
        }
    }

}