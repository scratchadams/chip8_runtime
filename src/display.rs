pub mod display {
    use minifb::{Window, WindowOptions, Key};
    use std::io::{Error, ErrorKind};
    use crate::proc::proc::{Proc, Registers};

    const WHITE: u32 = 0xFFFFFF;
    const BLACK: u32 = 0x000000;

    const LOGICAL_WIDTH: usize = 64;
    const LOGICAL_HEIGHT: usize = 32;
    const SCALE: usize = 10;

    const WIDTH: u32 = (LOGICAL_WIDTH * SCALE) as u32;
    const HEIGHT: u32 =(LOGICAL_HEIGHT * SCALE) as u32;

    fn get_bit(byte: u8, pos: u8) -> u8 {
        if(pos == 8) {
            return byte & 0x01;
        } 
        
       return ((byte >> (8-pos)) & 0x1) as u8;
    }

    pub struct DisplayWindow {
        pub window: Window,
        pub buf: Vec<u32>,
        pub key_state: u8,
    }

    impl DisplayWindow {
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
                window: window,
                buf: buf,
                key_state: 0xFF,
            })
        }

        pub fn clear_screen(&mut self) {
            self.buf
                .iter_mut()
                .for_each(|x| *x = 0);
        }

        pub fn draw_pixel(&mut self, x_pos: u32, y_pos: u32) {
            let x = x_pos % WIDTH;
            let y = y_pos % HEIGHT;
            let pos = (y * WIDTH + x) as usize;

            self.buf[pos] = WHITE;
        }

        pub fn draw_sprite(&mut self, regs: &mut Registers, mem: &mut [u8], x_pos: u32, y_pos: u32, var_z: usize) {
            let index = regs.I as usize;
            regs.V[0xF] = 0;

            for byte_index in 0..var_z {
                let byte = mem[index + byte_index];

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
            self
                .window
                .update_with_buffer(&self.buf, WIDTH as usize, HEIGHT as usize)
                .unwrap();
        }
    }

}