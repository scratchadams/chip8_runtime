pub mod display {
    use minifb::{Window, WindowOptions, Key};
    use std::io::{Error, ErrorKind};
    use crate::proc::proc::Proc;

    const WHITE: u32 = 0xFFFFFF;
    const BLACK: u32 = 0x000000;

    const WIDTH: u32 = 800;
    const HEIGHT: u32 = 600;

    fn get_bit(byte: u8, pos: u8) -> u8 {
        if(pos == 8) {
            return byte & 0x01;
        } 
        
       return ((byte >> (8-pos)) & 0x1) as u8;
    }

    pub struct DisplayWindow {
        pub window: Window,
        pub buf: Vec<u32>,
    }

    impl DisplayWindow {
        pub fn new() -> Result<DisplayWindow, Error> {
            let window = Window::new(
                "Chip8 Process", 
                WIDTH as usize, 
                HEIGHT as usize, 
                WindowOptions::default()
            ).unwrap();

            let buf: Vec<u32> = vec![0; (WIDTH * HEIGHT) as usize];
            
            Ok(DisplayWindow {
                window: window,
                buf: buf,
            })
        }

        pub fn draw_pixel(&mut self, x_pos: u32, y_pos: u32) {
            let x = x_pos % WIDTH;
            let y = y_pos % HEIGHT;
            let pos = (y * WIDTH + x) as usize;

            self.buf[pos] = WHITE;
        }

        pub fn draw_sprite(&mut self, proc: &mut Proc, x_pos: u32, y_pos: u32, var_z: usize) {
            let index = proc.regs.I as usize;
            proc.regs.V[0xF] = 0;

            for byte_index in 0..var_z {
                let byte = proc.mem[index + byte_index];

                for bit_index in 0..8 {
                    let x = (x_pos + bit_index) % WIDTH as u32;
                    let y = (y_pos + byte_index as u32) % HEIGHT as u32;
                    let pos = (y * WIDTH as u32 + x) as usize;

                    let sprite_pixel = (byte >> (7 - bit_index)) & 1;
                    let current_pixel = if self.buf[pos] == WHITE { 1 } else { 0 };
                    let new_pixel = current_pixel ^ sprite_pixel;

                    if current_pixel == 1 && new_pixel == 0 {
                        proc.regs.V[0xF] = 1;
                    }

                    self.buf[pos] = if new_pixel == 1 { WHITE } else { BLACK };
                }
            }
            self
                .window
                .update_with_buffer(&self.buf, WIDTH as usize, HEIGHT as usize)
                .unwrap();
        }
    }

}