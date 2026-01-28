pub mod display {
    use minifb::{Key, KeyRepeat, Window, WindowOptions};
    use std::collections::VecDeque;
    use std::io::Error;

    use chip8_core::device::device::DisplayDevice;
    pub use chip8_core::device::device::DisplayMode;
    use crate::proc::proc::Registers;

    const WHITE: u32 = 0xFFFFFF;
    const BLACK: u32 = 0x000000;

    const CHIP8_WIDTH: usize = 64;
    const CHIP8_HEIGHT: usize = 32;
    const CONSOLE_WIDTH: usize = 640;
    const CONSOLE_HEIGHT: usize = 320;
    pub const SCALE: usize = 2;
    pub const CHIP8_PIXEL_SCALE: usize = CONSOLE_WIDTH / CHIP8_WIDTH;

    const WINDOW_WIDTH: usize = CONSOLE_WIDTH * SCALE;
    const WINDOW_HEIGHT: usize = CONSOLE_HEIGHT * SCALE;

    const CELL_W: usize = 8;
    const CELL_H: usize = 8;
    const TEXT_COLS: usize = CONSOLE_WIDTH / CELL_W;
    const TEXT_ROWS: usize = CONSOLE_HEIGHT / CELL_H;

    #[derive(Clone)]
    struct Console {
        cols: usize,
        rows: usize,
        cursor_x: usize,
        cursor_y: usize,
        cells: Vec<u8>,
    }

    impl Console {
        fn new(cols: usize, rows: usize) -> Self {
            Self {
                cols,
                rows,
                cursor_x: 0,
                cursor_y: 0,
                cells: vec![b' '; cols * rows],
            }
        }

        fn clear(&mut self) {
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.cells.fill(b' ');
        }

        fn index(&self, col: usize, row: usize) -> usize {
            row * self.cols + col
        }

        fn put_char(&mut self, ch: u8) {
            if ch == b'\n' {
                self.cursor_x = 0;
                self.cursor_y += 1;
                if self.cursor_y >= self.rows {
                    self.scroll();
                }
                return;
            }

            if ch == b'\r' {
                self.cursor_x = 0;
                return;
            }

            if ch == 0x08 {
                self.backspace();
                return;
            }

            let idx = self.index(self.cursor_x, self.cursor_y);
            self.cells[idx] = ch;
            self.advance();
        }

        fn backspace(&mut self) {
            if self.cursor_x > 0 {
                self.cursor_x -= 1;
            } else if self.cursor_y > 0 {
                self.cursor_y -= 1;
                self.cursor_x = self.cols - 1;
            } else {
                return;
            }

            let idx = self.index(self.cursor_x, self.cursor_y);
            self.cells[idx] = b' ';
        }

        fn advance(&mut self) {
            self.cursor_x += 1;
            if self.cursor_x >= self.cols {
                self.cursor_x = 0;
                self.cursor_y += 1;
                if self.cursor_y >= self.rows {
                    self.scroll();
                }
            }
        }

        fn scroll(&mut self) {
            for row in 1..self.rows {
                for col in 0..self.cols {
                    let dst = self.index(col, row - 1);
                    let src = self.index(col, row);
                    self.cells[dst] = self.cells[src];
                }
            }
            for col in 0..self.cols {
                let idx = self.index(col, self.rows - 1);
                self.cells[idx] = b' ';
            }
            self.cursor_y = self.rows - 1;
            self.cursor_x = 0;
        }
    }

    pub struct DisplayWindow {
        pub window: Option<Window>,
        pub buf: Vec<u32>,
        pub key_state: u8,
        pub key_down: [bool; 16],
        pub last_key: Option<u8>,
        text_input: VecDeque<u8>,
        console: Console,
        mode: DisplayMode,
    }

    impl DisplayWindow {
        // buffer is pre-scaled (WINDOW_WIDTH x WINDOW_HEIGHT).
        pub fn new() -> Result<DisplayWindow, Error> {
            let mut window = Window::new(
                "Chip8 Process",
                WINDOW_WIDTH,
                WINDOW_HEIGHT,
                WindowOptions::default()
            ).unwrap();

            let buf: Vec<u32> = vec![0; WINDOW_WIDTH * WINDOW_HEIGHT];
            window
                .update_with_buffer(&buf, WINDOW_WIDTH, WINDOW_HEIGHT)
                .unwrap();
            
            Ok(DisplayWindow {
                window: Some(window),
                buf: buf,
                key_state: 0xFF,
                key_down: [false; 16],
                last_key: None,
                text_input: VecDeque::new(),
                console: Console::new(TEXT_COLS, TEXT_ROWS),
                mode: DisplayMode::Chip8,
            })
        }

        // headless display for tests or non-GUI runs.
        pub fn headless() -> DisplayWindow {
            DisplayWindow {
                window: None,
                buf: vec![0; WINDOW_WIDTH * WINDOW_HEIGHT],
                key_state: 0xFF,
                key_down: [false; 16],
                last_key: None,
                text_input: VecDeque::new(),
                console: Console::new(TEXT_COLS, TEXT_ROWS),
                mode: DisplayMode::Chip8,
            }
        }

        // build a display based on CHIP8_HEADLESS env var.
        pub fn from_env() -> Result<DisplayWindow, Error> {
            if std::env::var("CHIP8_HEADLESS").is_ok() {
                Ok(DisplayWindow::headless())
            } else {
                DisplayWindow::new()
            }
        }

        pub fn clear_screen(&mut self) {
            match self.mode {
                DisplayMode::Console => {
                    self.console.clear();
                    self.render_console();
                }
                DisplayMode::Chip8 => {
                    self.buf.iter_mut().for_each(|x| *x = 0);
                    if let Some(window) = self.window.as_mut() {
                        let _ = window.update_with_buffer(&self.buf, WINDOW_WIDTH, WINDOW_HEIGHT);
                    }
                }
            }
        }

        pub fn set_mode(&mut self, mode: DisplayMode) {
            if self.mode == mode {
                return;
            }
            self.mode = mode;
            self.text_input.clear();
            match self.mode {
                DisplayMode::Console => {
                    self.console.clear();
                    self.render_console();
                }
                DisplayMode::Chip8 => {
                    self.buf.iter_mut().for_each(|x| *x = 0);
                    if let Some(window) = self.window.as_mut() {
                        let _ = window.update_with_buffer(&self.buf, WINDOW_WIDTH, WINDOW_HEIGHT);
                    }
                }
            }
        }

        // poll_input captures the current pressed state of all 16 CHIP-8 keys.
        // example - if keys 0x3 and 0xA are both down, key_down[0x3] and
        // key_down[0xA] are true, and last_key becomes the first one found in the scan order.
        pub fn poll_input(&mut self, capture_text: bool) {
            let mapping: [(Key, u8); 16] = [
                (Key::Key1, 0x1), (Key::Key2, 0x2), (Key::Key3, 0x3), (Key::Key4, 0xC),
                (Key::Q,    0x4), (Key::W,    0x5), (Key::E,    0x6), (Key::R,    0xD),
                (Key::A,    0x7), (Key::S,    0x8), (Key::D,    0x9), (Key::F,    0xE),
                (Key::Z,    0xA), (Key::X,    0x0), (Key::C,    0xB), (Key::V,    0xF),
            ];

            // last_key reflects one currently-held key (not edge-triggered).
            let mut next_key_down = self.key_down;
            let mut next_last_key = None;
            let mut text_bytes = Vec::new();
            let capture_text = capture_text || self.mode == DisplayMode::Console;

            if let Some(window) = self.window.as_mut() {
                let _ = window.update();
                if capture_text {
                    text_bytes.extend(collect_text_input(window));
                }
                for (key, chip) in mapping {
                    let down = window.is_key_down(key);
                    next_key_down[chip as usize] = down;
                    if down && next_last_key.is_none() {
                        next_last_key = Some(chip);
                    }
                }
            } else {
                for (_, chip) in mapping {
                    if self.key_down[chip as usize] && next_last_key.is_none() {
                        next_last_key = Some(chip);
                    }
                }
            }

            self.key_down = next_key_down;
            self.last_key = next_last_key;
            if capture_text {
                self.text_input.extend(text_bytes);
            }

            // keep key_state for compatibility; 0xFF means "no key".
            self.key_state = self.last_key.unwrap_or(0xFF);
        }

        pub fn drain_text_input(&mut self) -> Vec<u8> {
            let mut data = Vec::with_capacity(self.text_input.len());
            while let Some(byte) = self.text_input.pop_front() {
                data.push(byte);
            }
            data
        }

        pub fn console_write(&mut self, data: &[u8]) {
            if self.mode != DisplayMode::Console {
                return;
            }
            for &byte in data {
                self.console.put_char(byte);
            }
            self.render_console();
        }

        pub fn console_backspace(&mut self) {
            if self.mode != DisplayMode::Console {
                return;
            }
            self.console.backspace();
            self.render_console();
        }

        // draw_sprite XORs sprite bits and sets VF on collision.
        pub fn draw_sprite(&mut self, regs: &mut Registers, sprite: &[u8], x_pos: u32, y_pos: u32) {
            if self.mode == DisplayMode::Console {
                return;
            }

            regs.V[0xF] = 0;

            for (byte_index, byte) in sprite.iter().enumerate() {
                let byte = *byte;

                for bit_index in 0..8 {
                    let sprite_pixel = (byte >> (7 - bit_index)) & 1;
                    if sprite_pixel == 0 {
                        continue;
                    }

                    let chip_x = ((x_pos as usize) + bit_index) % CHIP8_WIDTH;
                    let chip_y = ((y_pos as usize) + byte_index) % CHIP8_HEIGHT;
                    let base_x = chip_x * CHIP8_PIXEL_SCALE;
                    let base_y = chip_y * CHIP8_PIXEL_SCALE;

                    for dy in 0..CHIP8_PIXEL_SCALE {
                        for dx in 0..CHIP8_PIXEL_SCALE {
                            self.toggle_pixel(regs, base_x + dx, base_y + dy);
                        }
                    }
                }
            }

            if let Some(window) = self.window.as_mut() {
                let _ = window.update_with_buffer(&self.buf, WINDOW_WIDTH, WINDOW_HEIGHT);
            }
        }

        fn toggle_pixel(&mut self, regs: &mut Registers, logical_x: usize, logical_y: usize) {
            let phys_x = logical_x * SCALE;
            let phys_y = logical_y * SCALE;
            let pos = phys_y * WINDOW_WIDTH + phys_x;

            let current_pixel = if self.buf[pos] == WHITE { 1 } else { 0 };
            let new_pixel = current_pixel ^ 1;

            if current_pixel == 1 && new_pixel == 0 {
                regs.V[0xF] = 1;
            }

            let color = if new_pixel == 1 { WHITE } else { BLACK };
            for dy in 0..SCALE {
                for dx in 0..SCALE {
                    let scaled_x = phys_x + dx;
                    let scaled_y = phys_y + dy;
                    let idx = scaled_y * WINDOW_WIDTH + scaled_x;
                    self.buf[idx] = color;
                }
            }
        }
    }

    impl DisplayWindow {
        fn render_console(&mut self) {
            self.buf.iter_mut().for_each(|px| *px = BLACK);
            for row in 0..self.console.rows {
                for col in 0..self.console.cols {
                    let idx = self.console.index(col, row);
                    let ch = self.console.cells[idx];
                    self.draw_glyph(col, row, ch);
                }
            }

            if let Some(window) = self.window.as_mut() {
                let _ = window.update_with_buffer(&self.buf, WINDOW_WIDTH, WINDOW_HEIGHT);
                let text_bytes = collect_text_input(window);
                if !text_bytes.is_empty() {
                    self.text_input.extend(text_bytes);
                }
            }
        }

        fn draw_glyph(&mut self, col: usize, row: usize, ch: u8) {
            let glyph = glyph_for(ch);
            let base_x = col * CELL_W;
            let base_y = row * CELL_H;

            for (y, row_bits) in glyph.iter().enumerate() {
                for x in 0..CELL_W {
                    let bit = (row_bits >> (7 - x)) & 1;
                    let color = if bit == 1 { WHITE } else { BLACK };
                    let px = base_x + x;
                    let py = base_y + y;
                    for dy in 0..SCALE {
                        for dx in 0..SCALE {
                            let scaled_x = px * SCALE + dx;
                            let scaled_y = py * SCALE + dy;
                            let pos = scaled_y * WINDOW_WIDTH + scaled_x;
                            self.buf[pos] = color;
                        }
                    }
                }
            }
        }
    }

    impl DisplayDevice for DisplayWindow {
        fn poll_input(&mut self, capture_text: bool) {
            DisplayWindow::poll_input(self, capture_text);
        }

        fn clear_screen(&mut self) {
            DisplayWindow::clear_screen(self);
        }

        fn draw_sprite(&mut self, regs: &mut Registers, sprite: &[u8], x_pos: u32, y_pos: u32) {
            DisplayWindow::draw_sprite(self, regs, sprite, x_pos, y_pos);
        }

        fn is_key_down(&self, key: u8) -> bool {
            self.key_down
                .get(key as usize)
                .copied()
                .unwrap_or(false)
        }

        fn last_key(&self) -> Option<u8> {
            self.last_key
        }

        fn drain_text_input(&mut self) -> Vec<u8> {
            DisplayWindow::drain_text_input(self)
        }

        fn console_write(&mut self, data: &[u8]) {
            DisplayWindow::console_write(self, data);
        }

        fn console_backspace(&mut self) {
            DisplayWindow::console_backspace(self);
        }

        fn set_mode(&mut self, mode: DisplayMode) {
            DisplayWindow::set_mode(self, mode);
        }

        fn mode(&self) -> DisplayMode {
            self.mode
        }
    }

    fn glyph_for(ch: u8) -> [u8; 8] {
        let lower = match ch {
            b'A'..=b'Z' => ch + 32,
            _ => ch,
        };

        match lower {
            b'a' => pack([0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]),
            b'b' => pack([0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E]),
            b'c' => pack([0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E]),
            b'd' => pack([0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E]),
            b'e' => pack([0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F]),
            b'f' => pack([0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10]),
            b'g' => pack([0x0E, 0x11, 0x10, 0x10, 0x13, 0x11, 0x0F]),
            b'h' => pack([0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]),
            b'i' => pack([0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E]),
            b'j' => pack([0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C]),
            b'k' => pack([0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11]),
            b'l' => pack([0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F]),
            b'm' => pack([0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11]),
            b'n' => pack([0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11]),
            b'o' => pack([0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
            b'p' => pack([0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10]),
            b'q' => pack([0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D]),
            b'r' => pack([0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11]),
            b's' => pack([0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E]),
            b't' => pack([0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04]),
            b'u' => pack([0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
            b'v' => pack([0x11, 0x11, 0x11, 0x11, 0x0A, 0x0A, 0x04]),
            b'w' => pack([0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A]),
            b'x' => pack([0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11]),
            b'y' => pack([0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04]),
            b'z' => pack([0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F]),
            b'0' => pack([0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E]),
            b'1' => pack([0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E]),
            b'2' => pack([0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F]),
            b'3' => pack([0x0E, 0x11, 0x01, 0x06, 0x01, 0x11, 0x0E]),
            b'4' => pack([0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02]),
            b'5' => pack([0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E]),
            b'6' => pack([0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E]),
            b'7' => pack([0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08]),
            b'8' => pack([0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E]),
            b'9' => pack([0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C]),
            b' ' => pack([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
            b'-' => pack([0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00]),
            b'>' => pack([0x10, 0x08, 0x04, 0x02, 0x04, 0x08, 0x10]),
            b'<' => pack([0x02, 0x04, 0x08, 0x10, 0x08, 0x04, 0x02]),
            b':' => pack([0x00, 0x04, 0x00, 0x00, 0x04, 0x00, 0x00]),
            b'/' => pack([0x01, 0x02, 0x04, 0x08, 0x10, 0x00, 0x00]),
            b'.' => pack([0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C]),
            b'?' => pack([0x0E, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04]),
            _ => pack([0x1F, 0x11, 0x1B, 0x15, 0x1B, 0x11, 0x1F]),
        }
    }

    fn pack(rows: [u8; 7]) -> [u8; 8] {
        [
            rows[0] << 2,
            rows[1] << 2,
            rows[2] << 2,
            rows[3] << 2,
            rows[4] << 2,
            rows[5] << 2,
            rows[6] << 2,
            0x00,
        ]
    }

    fn collect_text_input(window: &mut Window) -> Vec<u8> {
        let mut text_bytes = Vec::new();
        let shift = window.is_key_down(Key::LeftShift) || window.is_key_down(Key::RightShift);
        for key in window.get_keys_pressed(KeyRepeat::Yes) {
            if let Some(byte) = key_to_ascii(key, shift) {
                text_bytes.push(byte);
            }
        }
        text_bytes
    }

    fn key_to_ascii(key: Key, shift: bool) -> Option<u8> {
        let base = match key {
            Key::Space => b' ',
            Key::Enter => b'\n',
            Key::Backspace => 0x08,
            Key::A => b'a',
            Key::B => b'b',
            Key::C => b'c',
            Key::D => b'd',
            Key::E => b'e',
            Key::F => b'f',
            Key::G => b'g',
            Key::H => b'h',
            Key::I => b'i',
            Key::J => b'j',
            Key::K => b'k',
            Key::L => b'l',
            Key::M => b'm',
            Key::N => b'n',
            Key::O => b'o',
            Key::P => b'p',
            Key::Q => b'q',
            Key::R => b'r',
            Key::S => b's',
            Key::T => b't',
            Key::U => b'u',
            Key::V => b'v',
            Key::W => b'w',
            Key::X => b'x',
            Key::Y => b'y',
            Key::Z => b'z',
            Key::Key0 => b'0',
            Key::Key1 => b'1',
            Key::Key2 => b'2',
            Key::Key3 => b'3',
            Key::Key4 => b'4',
            Key::Key5 => b'5',
            Key::Key6 => b'6',
            Key::Key7 => b'7',
            Key::Key8 => b'8',
            Key::Key9 => b'9',
            Key::Minus => b'-',
            Key::Equal => b'=',
            Key::LeftBracket => b'[',
            Key::RightBracket => b']',
            Key::Backslash => b'\\',
            Key::Semicolon => b';',
            Key::Apostrophe => b'\'',
            Key::Comma => b',',
            Key::Period => b'.',
            Key::Slash => b'/',
            _ => return None,
        };

        if !shift {
            return Some(base);
        }

        let shifted = match base {
            b'a'..=b'z' => base - 32,
            b'1' => b'!',
            b'2' => b'@',
            b'3' => b'#',
            b'4' => b'$',
            b'5' => b'%',
            b'6' => b'^',
            b'7' => b'&',
            b'8' => b'*',
            b'9' => b'(',
            b'0' => b')',
            b'-' => b'_',
            b'=' => b'+',
            b'[' => b'{',
            b']' => b'}',
            b'\\' => b'|',
            b';' => b':',
            b'\'' => b'\"',
            b',' => b'<',
            b'.' => b'>',
            b'/' => b'?',
            b'`' => b'~',
            other => other,
        };
        Some(shifted)
    }
}
