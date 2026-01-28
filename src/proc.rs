pub mod proc {
    pub use chip8_core::proc::proc::{ConsoleMode, InputMode, Registers};

    pub type Proc = chip8_core::proc::proc::Proc<crate::display::display::DisplayWindow>;
}
