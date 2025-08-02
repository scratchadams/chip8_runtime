pub mod chip8_engine {
    use crate::shared_memory::shared_memory::SharedMemory;
    use crate::proc::proc::Proc;
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

    pub fn opcode_0x0(mem: &mut SharedMemory, proc: &mut Proc, instruction: u16) {
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

    pub fn opcode_0x1(mem: &mut SharedMemory, proc: &mut Proc, instruction: u16) {
        proc.regs.PC = extract_nnn!(instruction);
    }

    //need to adjust to handle virtual addressing. maybe just keep a value in proc
    //struct to handle the pgd and pte indexes.
    pub fn opcode_0x2(mem: &mut SharedMemory, proc: &mut Proc, instruction: u16) {
        proc.regs.SP += 2;

        mem.phys_mem[proc.regs.SP as usize] = ((proc.regs.PC + 2) >> 8) as u8;
        mem.phys_mem[(proc.regs.SP + 1) as usize] = (proc.regs.PC + 2) as u8;

        proc.regs.PC = extract_nnn!(instruction);
    }

    pub fn opcode_0x3(mem: &mut SharedMemory, proc: &mut Proc, instruction: u16) {
        todo!("opcode 0x3");
    }


}