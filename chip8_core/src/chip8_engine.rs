pub mod chip8_engine {
    use crate::device::device::DisplayDevice;
    use crate::proc::proc::Proc;
    use crate::syscall::syscall::SyscallOutcome;
    use rand::Rng;
    use std::io::Error;
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
    /// each handler is responsible for advancing PC; the
    /// dispatch loop does not auto-increment.
    
    // keep opcode bit extraction consistent and centralized.
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

    pub fn opcode_0x0<F, D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16, mut dispatch_syscall: F) -> SyscallOutcome
    where
        F: FnMut(u16, &mut Proc<D>) -> Result<SyscallOutcome, Error>,
    {
        let nnn = extract_nnn!(instruction);

        match instruction {
            0x00e0 => {
                proc.display.clear_screen();
                proc.regs.PC += 2;
                SyscallOutcome::Completed
            },
            0x00ee => {
                // stack grows downward; SP points to top of stack.
                let val1 = proc.read_u8(proc.regs.SP as u32).unwrap() as u16;
                let val1 = val1 << 8;

                let val2 = proc.read_u8((proc.regs.SP + 1) as u32).unwrap() as u16;

                proc.regs.PC = val1 | val2;
                proc.regs.SP = proc.regs.SP.wrapping_add(2);
                SyscallOutcome::Completed
            },
            _ => {
                if (0x0100..0x0200).contains(&nnn) {
                    // syscall range is reserved for the host dispatcher.
                    match dispatch_syscall(nnn, proc) {
                        Ok(SyscallOutcome::Completed) => {
                            proc.regs.PC += 2;
                            SyscallOutcome::Completed
                        },
                        Ok(SyscallOutcome::Yielded) => {
                            proc.regs.PC += 2;
                            SyscallOutcome::Yielded
                        },
                        Ok(SyscallOutcome::Blocked) => {
                            proc.regs.PC += 2;
                            SyscallOutcome::Blocked
                        },
                        Err(_) => {
                            proc.regs.V[0xF] = 1;
                            proc.regs.V[0] = 0x01;
                            proc.regs.PC += 2;
                            SyscallOutcome::Completed
                        }
                    }
                } else {
                    proc.regs.PC += 2;
                    SyscallOutcome::Completed
                }
            }
        }
    }

    pub fn opcode_0x1<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
        proc.regs.PC = extract_nnn!(instruction);
    }

    // stack uses virtual addresses; translation handles paging.
    pub fn opcode_0x2<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
        // return address is stored as two bytes (hi/lo).
        let mut data: Vec<u8> = Vec::new();
        data.push(((proc.regs.PC + 2) >> 8) as u8);
        data.push((proc.regs.PC + 2) as u8);
        
        // write via virtual addresses to respect paging.
        proc.regs.SP = proc.regs.SP.wrapping_sub(2);
        proc.write_u8(proc.regs.SP as u32, data[0]).unwrap();
        proc.write_u8((proc.regs.SP + 1) as u32, data[1]).unwrap();

        proc.regs.PC = extract_nnn!(instruction);
    }

    pub fn opcode_0x3<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_kk = extract_kk!(instruction);

        if proc.regs.V[var_x as usize] == var_kk {
            proc.regs.PC = proc.regs.PC + 0x4;
        } else {
            proc.regs.PC = proc.regs.PC + 0x2;
        }
    }

    pub fn opcode_0x4<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_kk = extract_kk!(instruction);

        if proc.regs.V[var_x as usize] != var_kk {
            proc.regs.PC += 0x4;
        } else {
            proc.regs.PC += 0x2;
        }
    }

    pub fn opcode_0x5<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_y = extract_y!(instruction);

        if proc.regs.V[var_x as usize] == proc.regs.V[var_y as usize] {
            proc.regs.PC += 0x4;
        } else {
            proc.regs.PC += 0x2;
        }
    }

    pub fn opcode_0x6<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_kk = extract_kk!(instruction);

        proc.regs.V[var_x as usize] = var_kk;
        proc.regs.PC += 0x2;
    }

    pub fn opcode_0x7<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_kk = extract_kk!(instruction);

        proc.regs.V[var_x as usize] = proc.regs.V[var_x as usize].wrapping_add(var_kk);
        proc.regs.PC += 0x2;
    }

    pub fn opcode_0x8<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
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

                // handle overflow
                if temp2 > 0xFF {
                    proc.regs.V[0xF] = 1;
                } else {
                    proc.regs.V[0xF] = 0;
                }
                proc.regs.PC += 2;
            },
            0x05 => {
                let v_x = proc.regs.V[var_x] as i16;
                let v_y = proc.regs.V[var_y] as i16;
                let temp = v_x.wrapping_sub(v_y);
                proc.regs.V[var_x] = temp as u8;

                // handle borrow
                if v_x > v_y {
                    proc.regs.V[0xF] = 1;
                } else {
                    proc.regs.V[0xF] = 0;
                }
                proc.regs.PC += 2;
            },
            0x06 => {
                proc.regs.V[0xF] = proc.regs.V[var_x] & 1;
                proc.regs.V[var_x] = proc.regs.V[var_x] >> 1;
                proc.regs.PC += 2;
            },
            0x07 => {
                let v_x = proc.regs.V[var_x] as i16;
                let v_y = proc.regs.V[var_y] as i16;
                let temp = v_y.wrapping_sub(v_x);
                proc.regs.V[var_x] = temp as u8;

                // handle borrow
                if v_y > v_x {
                    proc.regs.V[0xF] = 1;
                } else {
                    proc.regs.V[0xF] = 0;
                }
                proc.regs.PC += 2;
            },
            0x0E => {
                proc.regs.V[0xF] = (proc.regs.V[var_x] & 0x80) >> 7;
                proc.regs.V[var_x] = proc.regs.V[var_x] << 1;
                proc.regs.PC += 2;
            },
            _ => {},
        }
    }

    pub fn opcode_0x9<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_y = extract_y!(instruction);

        if proc.regs.V[var_x as usize] != proc.regs.V[var_y as usize] {
            proc.regs.PC += 0x4;
        } else {
            proc.regs.PC += 0x2;
        }
    }

    pub fn opcode_0xA<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
        proc.regs.I = extract_nnn!(instruction);
        proc.regs.PC += 0x2;
    }

    pub fn opcode_0xB<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
        proc.regs.PC = extract_nnn!(instruction) + proc.regs.V[0] as u16;
    }

    pub fn opcode_0xC<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_kk = extract_kk!(instruction);

        let mut rng = rand::rng();
        let random_value: u8 = rng.random();

        proc.regs.V[var_x as usize] = random_value & var_kk;
        proc.regs.PC += 0x2;
    }

    pub fn opcode_0xD<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_y = extract_y!(instruction);
        let var_z = extract_z!(instruction);

        let x = proc.regs.V[var_x as usize] as u32;
        let y = proc.regs.V[var_y as usize] as u32;

        let addr = proc.regs.I as u32;
        let sprite = proc.read_bytes(addr, var_z as usize).unwrap();

        proc.display.draw_sprite(&mut proc.regs, &sprite, x, y);

        proc.regs.PC += 0x2;
    }

    pub fn opcode_0xE<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) {
        let var_x = extract_x!(instruction);
        let var_kk = extract_kk!(instruction);

        match var_kk {
            0x9E => {
                if proc.is_key_down(proc.regs.V[var_x as usize]) {
                    proc.regs.PC += 0x4;
                } else {
                    proc.regs.PC += 0x2;
                }
            },
            0xA1 => {
                if !proc.is_key_down(proc.regs.V[var_x as usize]) {
                    proc.regs.PC += 0x4;
                } else {
                    proc.regs.PC += 0x2;
                }
            },
            _ => {}
        }
    }

    pub fn opcode_0xF<D: DisplayDevice>(proc: &mut Proc<D>, instruction: u16) -> SyscallOutcome {
        let var_x = extract_x!(instruction);
        let var_kk = extract_kk!(instruction);

        match var_kk {
            0x07 => {
                proc.regs.V[var_x as usize] = proc.regs.DT;
                proc.regs.PC += 0x2;
                SyscallOutcome::Completed
            },
            0x0A => {
                if let Some(key) = proc.last_key() {
                    proc.regs.V[var_x as usize] = key;
                    proc.regs.PC += 0x2;
                    SyscallOutcome::Completed
                } else {
                    SyscallOutcome::Blocked
                }
            },
            0x15 => {
                proc.regs.DT = proc.regs.V[var_x as usize];
                proc.regs.PC += 0x2;
                SyscallOutcome::Completed
            },
            0x18 => {
                proc.regs.ST = proc.regs.V[var_x as usize];
                proc.regs.PC += 0x2;
                SyscallOutcome::Completed
            },
            0x1E => {
                proc.regs.I += proc.regs.V[var_x as usize] as u16;
                proc.regs.PC += 0x2;
                SyscallOutcome::Completed
            },
            0x29 => {
                let val = proc.regs.V[var_x as usize] as u16;
                proc.regs.I = val * 0x5;
                proc.regs.PC += 0x2;
                SyscallOutcome::Completed
            },
            0x33 => {
                let val = proc.regs.V[var_x as usize];
                let hundreds = val / 100;
                let tens = (val % 100) / 10;
                let ones = val % 10;
                proc.write_u8(proc.regs.I as u32, hundreds).unwrap();
                proc.write_u8((proc.regs.I + 1) as u32, tens).unwrap();
                proc.write_u8((proc.regs.I + 2) as u32, ones).unwrap();
                proc.regs.PC += 0x2;
                SyscallOutcome::Completed
            },
            0x55 => {
                for i in 0..=var_x {
                    let offset = proc.regs.I.wrapping_add(i as u16);
                    proc.write_u8(offset as u32, proc.regs.V[i as usize]).unwrap();
                }
                proc.regs.I = proc.regs.I.wrapping_add(var_x as u16 + 1);
                proc.regs.PC += 0x2;
                SyscallOutcome::Completed
            },
            0x65 => {
                for i in 0..=var_x {
                    let offset = proc.regs.I.wrapping_add(i as u16);
                    proc.regs.V[i as usize] = proc.read_u8(offset as u32).unwrap();
                }
                proc.regs.I = proc.regs.I.wrapping_add(var_x as u16 + 1);
                proc.regs.PC += 0x2;
                SyscallOutcome::Completed
            },
            _ => SyscallOutcome::Completed,
        }
    }
}
