#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

pub mod chip8_engine;
pub mod device;
pub mod proc;
pub mod shared_memory;
pub mod syscall;
