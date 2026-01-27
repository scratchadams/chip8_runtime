
mod shared_memory;
mod chip8_engine;
mod proc;
mod display;
mod kernel;

use kernel::kernel::Kernel;
use shared_memory::shared_memory::SharedMemory;
use std::sync::{Arc, Mutex};

use std::env;
use std::path::PathBuf;
fn main() {
    let mut args = env::args().skip(1);
    let mut root_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut roms: Vec<String> = Vec::new();

    while let Some(arg) = args.next() {
        if arg == "--root" {
            if let Some(path) = args.next() {
                root_dir = PathBuf::from(path);
            } else {
                eprintln!("--root requires a path");
                return;
            }
        } else {
            roms.push(arg);
        }
    }

    if roms.is_empty() {
        eprintln!("Usage: chip8_runtime --root <dir> <rom...>");
        return;
    }

    // Allocate system memory.
    let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
    let mut kernel = Kernel::new(mem, root_dir).unwrap();
    kernel.register_base_syscalls().unwrap();

    for rom in roms {
        let display = display::display::DisplayWindow::from_env().unwrap();
        kernel.spawn_proc_from_name(display, 1, &rom).unwrap();
    }

    let _ = kernel.run();
}
