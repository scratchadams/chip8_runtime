mod chip8_engine {
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
            $value = $value >> 0xc;
        };
    }

    macro_rules! extract_nnn {
        ($value:expr) => {
            $value = $value ^  extract_opcode!($value) as u16 << 0xc;
        };
    }

    macro_rules! extract_x {
        ($value:expr) => {
            (extract_nnn($value) >> 8) as u8;
        };
    }

    macro_rules! extract_kk {
        ($value:expr) => {
            extract_nnn($value) as u8;
        };
    }

    macro_rules! extract_y {
        ($value:expr) => {
            (extract_kk($value) >> 4) as u8;
        };
    }

    macro_rules! extract_z {
        ($value:expr) => {
            ((extract_kk($value) << 4) >> 4) as u8;
        };
    }


}