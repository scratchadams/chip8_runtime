pub mod proc {

    pub struct Registers {
        V: [u8; 16],
        DT: u8,
        ST: u8,
        I: u16,
        SP: u16,
        PC: u16,
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

        pub fn to_text(&self) -> String {
            let mut reg_string = "".to_string();

            for i in 0..self.V.len() {
                write!(reg_string, "V[{}] - {:x}\n",i, self.V[i])
                    .unwrap();
            }

            reg_string
        }

    }

    pub struct Proc {
        proc_id: u32,
        regs: Registers,
    }

}