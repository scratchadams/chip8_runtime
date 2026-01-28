pub mod device {
    use crate::proc::proc::Registers;

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum DisplayMode {
        Chip8,
        Console,
    }

    /// Minimal display/input surface required by the core interpreter.
    pub trait DisplayDevice {
        fn poll_input(&mut self, capture_text: bool);
        fn clear_screen(&mut self);
        fn draw_sprite(&mut self, regs: &mut Registers, sprite: &[u8], x_pos: u32, y_pos: u32);
        fn is_key_down(&self, key: u8) -> bool;
        fn last_key(&self) -> Option<u8>;
        fn drain_text_input(&mut self) -> Vec<u8>;
        fn console_write(&mut self, data: &[u8]);
        fn console_backspace(&mut self);
        fn set_mode(&mut self, mode: DisplayMode);
        fn mode(&self) -> DisplayMode;
    }
}
