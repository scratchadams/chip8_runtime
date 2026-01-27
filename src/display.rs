pub mod display {
    use minifb::{Window, WindowOptions, Key};
    use std::io::Error;
    use crate::proc::proc::Registers;

    const WHITE: u32 = 0xFFFFFF;
    const BLACK: u32 = 0x000000;

    const LOGICAL_WIDTH: usize = 64;
    const LOGICAL_HEIGHT: usize = 32;
    pub const SCALE: usize = 10;

    const WIDTH: u32 = (LOGICAL_WIDTH * SCALE) as u32;
    const HEIGHT: u32 =(LOGICAL_HEIGHT * SCALE) as u32;

    pub struct DisplayWindow {
        pub window: Option<Window>,
        pub buf: Vec<u32>,
        pub key_state: u8,
        pub key_down: [bool; 16],
        pub last_key: Option<u8>,
    }

    impl DisplayWindow {
        // buffer is pre-scaled (WIDTH x HEIGHT) to avoid per-frame resize.
        pub fn new() -> Result<DisplayWindow, Error> {
            let mut window = Window::new(
                "Chip8 Process", 
                WIDTH as usize, 
                HEIGHT as usize, 
                WindowOptions::default()
            ).unwrap();

            let buf: Vec<u32> = vec![0; (WIDTH * HEIGHT) as usize];
            window.update_with_buffer(&buf, WIDTH as usize, HEIGHT as usize).unwrap();
            
            Ok(DisplayWindow {
                window: Some(window),
                buf: buf,
                key_state: 0xFF,
                key_down: [false; 16],
                last_key: None,
            })
        }

        // headless display for tests or non-GUI runs.
        pub fn headless() -> DisplayWindow {
            DisplayWindow {
                window: None,
                buf: vec![0; (WIDTH * HEIGHT) as usize],
                key_state: 0xFF,
                key_down: [false; 16],
                last_key: None,
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
            // this clears the backing buffer; caller updates the window.
            self.buf
                .iter_mut()
                .for_each(|x| *x = 0);
        }

        // poll_input captures the current pressed state of all 16 CHIP-8 keys.
        // example - if keys 0x3 and 0xA are both down, key_down[0x3] and
        // key_down[0xA] are true, and last_key becomes the first one found in the scan order.
        pub fn poll_input(&mut self) {
            let mapping: [(Key, u8); 16] = [
                (Key::Key1, 0x1), (Key::Key2, 0x2), (Key::Key3, 0x3), (Key::Key4, 0xC),
                (Key::Q,    0x4), (Key::W,    0x5), (Key::E,    0x6), (Key::R,    0xD),
                (Key::A,    0x7), (Key::S,    0x8), (Key::D,    0x9), (Key::F,    0xE),
                (Key::Z,    0xA), (Key::X,    0x0), (Key::C,    0xB), (Key::V,    0xF),
            ];

            // last_key reflects one currently-held key (not edge-triggered).
            self.last_key = None;
            if let Some(window) = self.window.as_mut() {
                let _ = window.update();
                for (key, chip) in mapping {
                    let down = window.is_key_down(key);
                    self.key_down[chip as usize] = down;
                    if down && self.last_key.is_none() {
                        self.last_key = Some(chip);
                    }
                }
            } else {
                for (_, chip) in mapping {
                    if self.key_down[chip as usize] && self.last_key.is_none() {
                        self.last_key = Some(chip);
                    }
                }
            }

            // keep key_state for compatibility; 0xFF means "no key".
            self.key_state = self.last_key.unwrap_or(0xFF);
        }

        // draw_sprite XORs sprite bits and sets VF on collision.
        pub fn draw_sprite(&mut self, regs: &mut Registers, sprite: &[u8], x_pos: u32, y_pos: u32) {
            regs.V[0xF] = 0;

            for (byte_index, byte) in sprite.iter().enumerate() {
                let byte = *byte;

                for bit_index in 0..8 {
                    let x = (x_pos + bit_index) % LOGICAL_WIDTH as u32;
                    let y = (y_pos + byte_index as u32) % LOGICAL_HEIGHT as u32;

                    let sprite_pixel = (byte >> (7 - bit_index)) & 1;

                    for dy in 0..SCALE {
                        for dx in 0..SCALE {
                            let scaled_x = (x as usize * SCALE) + dx;
                            let scaled_y = (y as usize * SCALE) + dy;
                            let pos = scaled_y * WIDTH as usize + scaled_x;

                            let current_pixel = if self.buf[pos] == WHITE { 1 } else { 0 };
                            let new_pixel = current_pixel ^ sprite_pixel;

                            if current_pixel == 1 && new_pixel == 0 {
                                regs.V[0xF] = 1;
                            }

                            self.buf[pos] = if new_pixel == 1 { WHITE } else { BLACK };

                            /*self
                                .window
                                .update_with_buffer(&self.buf, WIDTH as usize, HEIGHT as usize)
                                .unwrap();*/
                        }
                    }
                }
            }
            if let Some(window) = self.window.as_mut() {
                window
                    .update_with_buffer(&self.buf, WIDTH as usize, HEIGHT as usize)
                    .unwrap();
            }
        }
    }

}
