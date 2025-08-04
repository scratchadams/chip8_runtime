pub mod display {
    use minifb::{Window, WindowOptions, Key};
    use std::io::{Error, ErrorKind};

    const WHITE: u32 = 0xFFFFFF;
    const WIDTH: u32 = 800;
    const HEIGHT: u32 = 600;

    pub struct DisplayWindow {
        window: Window,
        buf: Vec<u32>,
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
            let index = (y * WIDTH + x) as usize;

            self.buf[index] = WHITE;
        }
    }

}