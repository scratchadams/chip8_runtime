pub mod chip8_engine {
    use crate::shared_memory::shared_memory::SharedMemory;
    use crate::proc::proc::Proc;
    use rand::Rng;
    /// To handle the chip8 instruction set, we will define a handler
    /// function for each first nibble (i.e - 0x0, 0x1, 0x2, etc...)
    /// any nibble which has multiple instructions associated with it
    /// will be handled within that top-level instruction handler.
    /// 
    /// We will define helper macros to extract the first nibble for
    /// function dispatch, as well as the remainder of instruction/variable
    /// options. (i.e - NNN, X, KK, Y, Z according to chip8 spec). The first
    /// nibble will always be extracted at the beginning of the event loop
    /// while additional variables will be extracted based on the needs of 
    /// each instruction handler.
    
    macro_rules! extract_opcode {
        ($value:expr) => {
            $value = ($value >> 0xc) as u8
        };
    }

    macro_rules! extract_nnn {
        ($value:expr) => {
            ($value & 0x0FFF) as u16
        };
    }

    macro_rules! extract_x {
        ($value:expr) => {
            (($value >> 0x8) & 0xF) as u8
        };
    }

    macro_rules! extract_kk {
        ($value:expr) => {
            ($value & 0xFF) as u8
        };
    }

    macro_rules! extract_y {
        ($value:expr) => {
            (($value >> 0x4) & 0xF) as u8
        };
    }

    macro_rules! extract_z {
        ($value:expr) => {
            ($value & 0xF) as u8
        };
    }

    pub fn opcode_0x0(proc: &mut Proc, instruction: u16) {
        let opt = extract_kk!(instruction);

        match opt {
            0xe0 => {
                todo!("clear screen");
            },
            0xee => {
                todo!("return");
            },
            _ => {
                todo!("do nothing, but incremement stack pointer");
            }
        }
    }

    pub fn opcode_0x1(proc: &mut Proc, instruction: u16) {
        proc.regs.PC = extract_nnn!(instruction);
    }

    //need to adjust to handle virtual addressing. maybe just keep a value in proc
    //struct to handle the pgd and pte indexes.
    pub fn opcode_0x2(proc: &mut Proc, instruction: u16) {
        proc.regs.SP += 2;

        proc.mem[proc.regs.SP as usize] = ((proc.regs.PC + 2) >> 8) as u8;
        proc.mem[(proc.regs.SP + 1) as usize] = (proc.regs.PC + 2) as u8;

        proc.regs.PC = extract_nnn!(instruction);
    }

    pub fn opcode_0x3(proc: &mut Proc, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_kk = extract_kk!(instruction);

        if proc.regs.V[var_x as usize] == var_kk {
            proc.regs.PC = proc.regs.PC + 0x4;
        } else {
            proc.regs.PC = proc.regs.PC + 0x2;
        }
    }

    pub fn opcode_0x4(proc: &mut Proc, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_kk = extract_kk!(instruction);

        if proc.regs.V[var_x as usize] != var_kk {
            proc.regs.PC += 0x4;
        } else {
            proc.regs.PC += 0x2;
        }
    }

    pub fn opcode_0x5(proc: &mut Proc, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_y = extract_y!(instruction);

        if var_x == var_y {
            proc.regs.PC += 0x4;
        } else {
            proc.regs.PC += 0x2;
        }
    }

    pub fn opcode_0x6(proc: &mut Proc, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_kk = extract_kk!(instruction);

        proc.regs.V[var_x as usize] = var_kk;
        proc.regs.PC += 0x2;
    }

    pub fn opcode_0x7(proc: &mut Proc, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_kk = extract_kk!(instruction);

        proc.regs.V[var_x as usize] = proc.regs.V[var_x as usize].wrapping_add(var_kk);
        proc.regs.PC += 0x2;
    }

    pub fn opcode_0x8(proc: &mut Proc, instruction: u16) {
        let var_z = extract_z!(instruction);
        let var_x = extract_x!(instruction) as usize;
        let var_y = extract_y!(instruction) as usize;

        match var_z {
            0x00 => {
                proc.regs.V[var_x] = proc.regs.V[var_y];
                proc.regs.PC += 2;
            },
            0x01 => {
                proc.regs.V[var_x] = proc.regs.V[var_x] | proc.regs.V[var_y];
                proc.regs.PC += 2;
            },
            0x02 => {
                proc.regs.V[var_x] = proc.regs.V[var_x] & proc.regs.V[var_y];
                proc.regs.PC += 2;
            },
            0x03 => {
                proc.regs.V[var_x] = proc.regs.V[var_x] ^ proc.regs.V[var_y];
                proc.regs.PC += 2;
            },
            0x04 => {
                let v_x = proc.regs.V[var_x] as u16;
                let v_y = proc.regs.V[var_y] as u16;

                let temp = v_x.wrapping_add(v_y);
                let temp2 = (proc.regs.V[var_x] as u32 + proc.regs.V[var_y] as u32) as u32;

                proc.regs.V[var_x] = temp as u8;

                if temp2 > 255 {
                    proc.regs.V[0xF] = 1;
                } else {
                    proc.regs.V[0xF] = 0;
                }

                proc.regs.PC += 2;
            },
            0x05 => {
                let v_x = proc.regs.V[var_x];
                let v_y = proc.regs.V[var_y];

                let temp = v_x.wrapping_sub(v_y);

                proc.regs.V[var_x] = temp;

                if v_x > v_y {
                    proc.regs.V[0xF] = 1;
                } else {
                    proc.regs.V[0xF] = 0;
                }

                proc.regs.PC += 0x2;
            },
            0x06 => {
                let v_x = proc.regs.V[var_x];
                let v_y = proc.regs.V[var_y];

                let temp = proc.regs.V[var_x] / 2;
                proc.regs.V[var_x] = temp;

                if (v_x & 1) == 1 {
                    proc.regs.V[0xF] = 1;
                } else {
                    proc.regs.V[0xF] = 0;
                }

                proc.regs.PC += 0x2;
            },
            0x07 => {
                let v_x = proc.regs.V[var_x];
                let v_y = proc.regs.V[var_y];

                let temp = proc.regs.V[var_y].wrapping_sub(proc.regs.V[var_x]);
                proc.regs.V[var_x] = temp;

                if v_y >= v_x {
                    proc.regs.V[0xF] = 1;
                } else {
                    proc.regs.V[0xF] = 0;
                }

                proc.regs.PC += 0x2;
            },
            0x0E => {
                let v_x = proc.regs.V[var_x];
                //let v_y = proc.regs.V[var_y];

                let temp = proc.regs.V[var_x].wrapping_mul(2);
                proc.regs.V[var_x] = temp;

                if (v_x & 0x80) == 0x80 {
                    proc.regs.V[0xF] = 1;
                } else {
                    proc.regs.V[0xF] = 0;
                }

                proc.regs.PC += 0x2;
            },
            _ => {
                proc.regs.PC += 0x2;
            },
        }
    }

    pub fn opcode_0x9(proc: &mut Proc, instruction: u16) {
        let var_x = extract_x!(instruction) as usize;
        let var_y = extract_y!(instruction) as usize;

        if proc.regs.V[var_x] != proc.regs.V[var_y] {
            proc.regs.PC += 4;
        } else {
            proc.regs.PC += 2;
        }
    }

    pub fn opcode_0xa(proc: &mut Proc, instruction: u16) {
        proc.regs.I = extract_nnn!(instruction);

        proc.regs.PC += 2;
    }

    pub fn opcode_0xb(proc: &mut Proc, instruction: u16) {
        proc.regs.PC = extract_nnn!(instruction) + (proc.regs.V[0] as u16);
    }

    pub fn opcode_0xc(proc: &mut Proc, instruction: u16) {
        let mut rng = rand::rng();
        let rnd: u8 = rng.random();

        let var_x = extract_x!(instruction) as usize;
        let var_kk = extract_kk!(instruction);

        proc.regs.V[var_x] = rnd & var_kk;
        proc.regs.PC += 2;
    }

    pub fn opcode_0xd(proc: &mut Proc, instruction: u16) {
        todo!("implement display functionality");
    }

    pub fn opcode_0xe(proc: &mut Proc, instruction: u16) {
        let var_x = extract_x!(instruction) as usize;
        let var_kk = extract_kk!(instruction);

        match var_kk {
            0x9e => {
                if proc.regs.V[var_x] == proc.current_key {
                    proc.regs.PC += 4;
                } else {
                    proc.regs.PC += 2;
                }
            },
            0xa1 => {
                if proc.regs.V[var_x] != proc.current_key {
                    proc.regs.PC += 4;
                } else {
                    proc.regs.PC += 2;
                }
            },
            _ => {
                proc.regs.PC += 2;
            },
        }

    }

    pub fn opcode_0xf(proc: &mut Proc, instruction: u16) {
        let var_x = extract_x!(instruction) as usize;
        let var_kk = extract_kk!(instruction);

        match var_kk {
            0x07 => {
                proc.regs.V[var_x] = proc.regs.DT;
                proc.regs.PC += 2;
            },
            0x0a => {
                proc.regs.V[var_x] = proc.current_key;
                if proc.current_key != 0xFF {
                    proc.regs.PC += 2;
                }
            },
            0x15 => {
                proc.regs.DT = proc.regs.V[var_x];
                proc.regs.PC += 2;
            },
            0x18 => {
                proc.regs.ST = proc.regs.V[var_x];
                proc.regs.PC += 2;
            },
            0x1e => {
                proc.regs.I += proc.regs.V[var_x] as u16;
                proc.regs.PC += 2;
            },
            0x29 => {
                proc.regs.I = proc.mem[var_x * 5] as u16;
                proc.regs.PC += 2;
            },
            0x33 => {
                let mut dec: u8 = proc.regs.V[var_x];
                
                proc.mem[(proc.regs.I + 2) as usize] = dec % 10;
                dec = dec / 10;

                proc.mem[(proc.regs.I + 1) as usize] = dec % 10;
                dec = dec / 10;

                proc.mem[proc.regs.I as usize] = dec % 10;                
            },
            0x55 => {
                for i in 0..=var_x {
                    proc.mem[(proc.regs.I + (i as u16)) as usize] = proc.regs.V[i as usize];
                }
                proc.regs.PC += 2;
            },
            0x65 => {
                for i in 0..=var_x {
                    proc.regs.V[i as usize] = proc.mem[(proc.regs.I + (i as u16)) as usize];
                }
                proc.regs.PC += 2;
            },
            _ => {
                println!("Unknown 0xF Instruction {:x} at position {:x}", instruction, proc.regs.PC);
                proc.regs.PC += 2;
            },
        }
    }
}