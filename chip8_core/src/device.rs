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
        fn present_if_due(&mut self);
        fn is_key_down(&self, key: u8) -> bool;
        fn last_key(&self) -> Option<u8>;
        fn drain_text_input(&mut self) -> Vec<u8>;
        fn console_write(&mut self, data: &[u8]);
        fn console_backspace(&mut self);
        fn set_mode(&mut self, mode: DisplayMode);
        fn mode(&self) -> DisplayMode;
        fn refresh_hz(&self) -> u32;
        fn set_refresh_hz(&mut self, chip8_hz: u32, console_hz: u32);
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum InputError {
        Io,
    }

    /// Minimal host input interface for kernel-managed text input.
    pub trait InputDevice {
        fn push_input(&mut self, data: &[u8]);
        fn blocking_read_line(&mut self) -> Result<(), InputError>;
        fn blocking_read_byte(&mut self) -> Result<(), InputError>;
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum FsError {
        Invalid,
        Io,
        NotFound,
        NotDir,
        IsDir,
        NameTooLong,
        TooManyOpen,
        Path,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum FsEntryKind {
        File,
        Dir,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct FsEntry {
        pub name: String,
        pub kind: FsEntryKind,
        pub size: u32,
    }

    /// Minimal filesystem surface for host-backed ROM access.
    pub trait FsDevice {
        fn list(&mut self, path: &str, max_entries: usize) -> Result<Vec<FsEntry>, FsError>;
        fn open(&mut self, pid: u32, path: &str) -> Result<u8, FsError>;
        fn read(&mut self, pid: u32, fd: u8, len: usize) -> Result<Vec<u8>, FsError>;
        fn close(&mut self, pid: u32, fd: u8) -> Result<(), FsError>;
    }
}
