pub mod proc {
    use std::io::Error;
    use std::collections::HashMap;
    use std::fs;
    use std::mem::size_of;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use crate::chip8_engine::chip8_engine::*;
    use crate::shared_memory;
    use crate::shared_memory::shared_memory::SharedMemory;
    use crate::display::display::DisplayWindow;

    macro_rules! extract_opcode {
        ($value:expr) => {
            ($value >> 0xc) as u8
        };
    }

    // Codex generated: CHIP-8 font sprites are 4x5 pixels, 5 bytes per glyph (0-F).
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
        pub procs: HashMap<u32, Proc<'a>>,
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
        pub mem: &'a mut Arc<Mutex<SharedMemory>>,
        pub display: DisplayWindow,
        pub base_addr: u32,
        last_timer_tick: Instant,
    }

    impl<'a> Proc<'a> {
        /// This process will return a new Proc (process) object
        /// A page of memory is mmaped to provide the virtual mapping
        /// this process will use.
        /// 
        /// A new display window is created per-process and associated
        /// with the Proc object
        /// Codex generated: base_addr is the physical offset for this process'
        /// 4KB page; PC and I remain CHIP-8 virtual addresses within that page.
        pub fn new(mem: &'a mut Arc<Mutex<SharedMemory>>) -> Result<Proc<'a>, Error> {
            let display = DisplayWindow::new().unwrap();
            Proc::new_with_display(mem, display)
        }

        // Codex generated: helper for tests and alternate frontends to supply a display implementation.
        pub fn new_with_display(
            mem: &'a mut Arc<Mutex<SharedMemory>>,
            display: DisplayWindow,
        ) -> Result<Proc<'a>, Error> {
            let vaddr = mem.lock()
                .unwrap()
                .mmap(1)?;

            Ok(Proc {
                proc_id: 0x41,
                regs: Registers::default(),
                mem: mem,
                display: display,
                base_addr: vaddr,
                last_timer_tick: Instant::now(),
            })
        }

        // Codex generated: headless constructor used by tests to avoid opening a window.
        pub fn new_headless(mem: &'a mut Arc<Mutex<SharedMemory>>) -> Result<Proc<'a>, Error> {
            let display = DisplayWindow::new_headless().unwrap();
            Proc::new_with_display(mem, display)
        }

        /// This function loads chip8 program text from a file
        /// and loads it into the process' memory space
        /// 
        /// Codex generated: sprites are loaded at the base of the process page,
        /// while program bytes start at 0x200 per CHIP-8 convention.
        pub fn load_program(&mut self, filename: String) -> Result<(), Error> {
            let program_text = fs::read(filename)?;
            if program_text.len() > shared_memory::shared_memory::PAGE_SIZE - 0x200 {
                return Err(Error::new(std::io::ErrorKind::FileTooLarge, "File too large"));
            }

            //copy sprites into process memory
            //self.mem[0x0..0x50].copy_from_slice(&chip8_sprites);
            let sprite_addr = self.base_addr as usize + 0x0;
            let sprite_vec = chip8_sprites.to_vec();
            let _ = self.mem.lock()
                .unwrap()
                .write(sprite_addr, &sprite_vec, sprite_vec.len());

            //copy program text into process memory
            //self.mem.lock().unwrap()[0x200..(0x200 + program_text.len())].copy_from_slice(&program_text);
            let prog_addr = self.base_addr as usize + 0x200;
            let _ = self.mem.lock()
                .unwrap()
                .write(prog_addr, &program_text, program_text.len());

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
        /// Codex generated: the loop is intentionally tight; timers and exit
        /// conditions are expected to be integrated externally.
        pub fn run_program(&mut self) {
            // Codex generated: classic fetch-decode-execute loop for CHIP-8.
            loop {
                self.step();
            }
        }

        // Codex generated: execute a single CHIP-8 instruction for test-driven stepping.
        pub fn step(&mut self) {
            // Codex generated: poll input each cycle so Ex9E/ExA1/Fx0A see live key states.
            self.display.poll_input();
            self.tick_timers();

            let pc = self.regs.PC as usize;
            
            let addr1 = self.base_addr as usize + pc;
            let addr2 = self.base_addr as usize + (pc + 1);
            
            // Codex generated: opcodes are big-endian in memory (hi byte then lo byte).
            let val1 = self.mem
                .lock()
                .unwrap()
                .read(addr1, size_of::<u8>())
                .unwrap()[0];
            let val1 = (val1 as u16) << 8;

            let val2 = self.mem
                .lock()
                .unwrap()
                .read(addr2, size_of::<u8>())
                .unwrap()[0];
            let val2 = val2 as u16;

            let instruction = val1 | val2;

            //let instruction = ((self.mem.lock().unwrap().phys_mem[pc] as u16) << 8) | self.mem.lock().unwrap().phys_mem[pc+1] as u16;
            
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

        // Codex generated: decrement DT/ST at ~60Hz based on wall-clock time.
        fn tick_timers(&mut self) {
            let tick = Duration::from_micros(1_000_000 / 60);
            let now = Instant::now();
            let elapsed = now.duration_since(self.last_timer_tick);
            if elapsed < tick {
                return;
            }

            let ticks = (elapsed.as_nanos() / tick.as_nanos()) as u32;
            let dec = ticks.min(u8::MAX as u32) as u8;

            self.regs.DT = self.regs.DT.saturating_sub(dec);
            self.regs.ST = self.regs.ST.saturating_sub(dec);
            self.last_timer_tick = self.last_timer_tick + (tick * ticks);
        }
    }

}
