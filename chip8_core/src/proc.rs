pub mod proc {
    use std::collections::VecDeque;
    use std::io::Error;
    use std::mem::size_of;
    use std::sync::{Arc, Mutex};

    use crate::chip8_engine::chip8_engine::*;
    use crate::device::device::DisplayDevice;
    use crate::syscall::syscall::SyscallOutcome;
    use crate::shared_memory;
    use crate::shared_memory::shared_memory::SharedMemory;

    macro_rules! extract_opcode {
        ($value:expr) => {
            ($value >> 0xc) as u8
        };
    }

    // CHIP-8 font sprites are 4x5 pixels, 5 bytes per glyph (0-F).
    const CHIP8_SPRITES: [u8; 80] = [
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


    #[allow(non_snake_case)]
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
                SP: 0,
                PC: 0x200,
            }
        }
    }

    impl Registers {}

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum InputMode {
        Line,
        Byte,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum ConsoleMode {
        Host,
        Display,
    }

    pub struct Proc<D: DisplayDevice> {
        pub regs: Registers,
        pub mem: Arc<Mutex<SharedMemory>>,
        pub display: D,
        pub page_table: Vec<u32>,
        pub vm_size: u32,
        pub input_mode: InputMode,
        pub console_mode: ConsoleMode,
        pub console_input: VecDeque<u8>,
    }

    impl<D: DisplayDevice> Proc<D> {
        // constructor with an explicit page count for multi-page VMs.
        pub fn new_with_display_and_pages(
            mem: Arc<Mutex<SharedMemory>>,
            display: D,
            pages: u16,
        ) -> Result<Proc<D>, Error> {
            let page_table = mem
                .lock()
                .unwrap()
                .mmap(pages)?;
            let vm_size = pages as u32 * shared_memory::shared_memory::PAGE_SIZE as u32;
            let mut regs = Registers::default();
            regs.SP = vm_size.min(u16::MAX as u32) as u16;
            Ok(Proc {
                regs: regs,
                mem: mem,
                display: display,
                page_table: page_table,
                vm_size: vm_size,
                input_mode: InputMode::Line,
                console_mode: ConsoleMode::Host,
                console_input: VecDeque::new(),
            })
        }

        // translate a virtual address into a physical address.
        pub fn translate(&self, vaddr: u32) -> Result<usize, Error> {
            if vaddr >= self.vm_size {
                return Err(Error::new(std::io::ErrorKind::Other, "virtual address out of range"));
            }

            let page = (vaddr as usize) / shared_memory::shared_memory::PAGE_SIZE;
            let offset = (vaddr as usize) % shared_memory::shared_memory::PAGE_SIZE;
            let phys_base = *self.page_table
                .get(page)
                .ok_or_else(|| Error::new(std::io::ErrorKind::Other, "page table index out of range"))? as usize;

            Ok(phys_base + offset)
        }

        // read a single byte using virtual addressing.
        pub fn read_u8(&mut self, vaddr: u32) -> Result<u8, Error> {
            let phys = self.translate(vaddr)?;
            Ok(self.mem
                .lock()
                .unwrap()
                .read(phys, size_of::<u8>())?
                [0])
        }

        // write a single byte using virtual addressing.
        pub fn write_u8(&mut self, vaddr: u32, value: u8) -> Result<(), Error> {
            let phys = self.translate(vaddr)?;
            let data = vec![value];
            self.mem
                .lock()
                .unwrap()
                .write(phys, &data, data.len())
        }

        // write a byte slice across page boundaries if needed.
        pub fn write_bytes(&mut self, vaddr: u32, data: &[u8]) -> Result<(), Error> {
            for (idx, byte) in data.iter().enumerate() {
                let addr = vaddr
                    .checked_add(idx as u32)
                    .ok_or_else(|| Error::new(std::io::ErrorKind::Other, "overflow computing write address"))?;
                self.write_u8(addr, *byte)?;
            }
            Ok(())
        }

        // read a byte slice across page boundaries if needed.
        pub fn read_bytes(&mut self, vaddr: u32, len: usize) -> Result<Vec<u8>, Error> {
            let mut data = Vec::with_capacity(len);
            for idx in 0..len {
                let addr = vaddr
                    .checked_add(idx as u32)
                    .ok_or_else(|| Error::new(std::io::ErrorKind::Other, "overflow computing read address"))?;
                data.push(self.read_u8(addr)?);
            }
            Ok(data)
        }

        // read a 16-bit big-endian value using virtual addressing.
        pub fn read_u16(&mut self, vaddr: u32) -> Result<u16, Error> {
            let hi = self.read_u8(vaddr)? as u16;
            let lo = self.read_u8(vaddr + 1)? as u16;
            Ok((hi << 8) | lo)
        }

        /// Load chip8 program bytes into the process' memory space.
        ///
        /// sprites are loaded at the base of the process page,
        /// while program bytes start at 0x200 per CHIP-8 convention.
        pub fn load_program_bytes(&mut self, program: &[u8]) -> Result<(), Error> {
            let max_size = self.vm_size as usize - 0x200;
            if program.len() > max_size {
                return Err(Error::new(std::io::ErrorKind::FileTooLarge, "File too large"));
            }

            //copy sprites into process memory
            //self.mem[0x0..0x50].copy_from_slice(&chip8_sprites);
            let sprite_vec = CHIP8_SPRITES.to_vec();
            self.write_bytes(0x0, &sprite_vec)?;

            //copy program text into process memory
            //self.mem.lock().unwrap()[0x200..(0x200 + program_text.len())].copy_from_slice(&program_text);
            self.write_bytes(0x200, program)?;

            Ok(())
        }

        /// This method is responsible for running the loaded ch8 program.
        /// It starts a loop that initially sets the program counter, grabs
        /// values relevant to the offset of the PC from the program's memory 
        /// space to create an instruction.
        /// 
        /// The initial opcode is extracted from the instruction and used to call
        /// the various opcode handlers. The instruction value and Proc object are 
        /// passed to the matched handler to execute the instruction. 
        /// the loop is intentionally tight; timers and exit
        /// conditions are expected to be integrated externally.
        /// `ticks` is the number of 60Hz timer ticks supplied by the kernel.
        // execute a single CHIP-8 instruction for test-driven stepping.
        pub fn step<F>(&mut self, ticks: u32, mut dispatch_syscall: F) -> SyscallOutcome
        where
            F: FnMut(u16, &mut Proc<D>) -> Result<SyscallOutcome, Error>,
        {
            // poll input each cycle so Ex9E/ExA1/Fx0A see live key states.
            // text capture is handled by the kernel when a proc opts into console mode.
            self.display.poll_input(false);
            self.tick_timers(ticks);

            let pc = self.regs.PC as usize;
            
            // opcodes are big-endian in memory (hi byte then lo byte).
            let val1 = self.read_u8(pc as u32).unwrap() as u16;
            let val1 = val1 << 8;
            let val2 = self.read_u8((pc + 1) as u32).unwrap() as u16;
            
            let instruction = val1 | val2;
            let opcode = extract_opcode!(instruction);

            match opcode {
                0x0 => {
                    return opcode_0x0(self, instruction, &mut dispatch_syscall);
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
                    opcode_0xA(self, instruction);
                },
                0xB => {
                    opcode_0xB(self, instruction);
                },
                0xC => {
                    opcode_0xC(self, instruction);
                },
                0xD => {
                    opcode_0xD(self, instruction);
                },
                0xE => {
                    opcode_0xE(self, instruction);
                },
                0xF => {
                    return opcode_0xF(self, instruction);
                },
                _ => {},
            }

            // timers + pc advance are handled in opcode handlers.
            SyscallOutcome::Completed
        }

        pub fn is_key_down(&self, key: u8) -> bool {
            self.display.is_key_down(key)
        }

        pub fn last_key(&self) -> Option<u8> {
            self.display.last_key()
        }

        /// decrement DT/ST by the number of 60Hz ticks supplied by the kernel.
        fn tick_timers(&mut self, ticks: u32) {
            let dec = ticks.min(u8::MAX as u32) as u8;
            self.regs.DT = self.regs.DT.saturating_sub(dec);
            self.regs.ST = self.regs.ST.saturating_sub(dec);
        }
    }
}
