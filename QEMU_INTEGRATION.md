# QEMU and Embedded Target Integration Guide

This document explains how to use `chip8_core` in QEMU, bare-metal embedded systems, or other `no_std` environments.

---

## Overview

As of Week 2 (Phase -1), `chip8_core` is fully portable to `no_std` targets. The core interpreter can run on:
- QEMU ARM/RISC-V virtual machines
- Bare-metal microcontrollers (STM32, ESP32, etc.)
- Custom embedded systems
- Hosted environments (existing std support)

**Key abstractions enabling portability:**
1. **MemoryAllocator trait** - swap heap allocation for fixed-size arrays
2. **DisplayDevice trait** - route display to framebuffer or UART
3. **InputDevice trait** - route I/O to UART or other serial device
4. **no_std compatibility** - core uses only `alloc` (Vec, Arc) not `std`

---

## Prerequisites

### For QEMU ARM Example

```bash
# Install Rust embedded toolchain
rustup target add thumbv7em-none-eabihf

# Install QEMU ARM emulator
sudo apt-get install qemu-system-arm

# Install cargo-binutils for binary manipulation
cargo install cargo-binutils
rustup component add llvm-tools-preview
```

### For Other Targets

- **RISC-V**: `rustup target add riscv32imac-unknown-none-elf`
- **STM32**: `rustup target add thumbv7m-none-eabi`
- **ESP32**: Follow [esp-rs docs](https://esp-rs.github.io/book/)

---

## Step 1: Configure chip8_core for no_std

In your embedded project's `Cargo.toml`:

```toml
[dependencies]
chip8_core = { path = "../chip8_core", default-features = false }
# Or from git:
# chip8_core = { git = "https://github.com/scratchadams/chip8_runtime", default-features = false }
```

**Important**: `default-features = false` disables the `std` feature, enabling `no_std` mode.

---

## Step 2: Implement MemoryAllocator

For embedded targets without heap, use a fixed-size static array:

```rust
use chip8_core::device::device::{MemoryAllocator, AllocError};

const PHYS_MEM_SIZE: usize = 0x10000; // 64KB
const PAGE_SIZE: usize = 0x1000;      // 4KB pages
const PAGE_COUNT: usize = PHYS_MEM_SIZE / PAGE_SIZE;

pub struct FixedArena {
    memory: [u8; PHYS_MEM_SIZE],
    bitmap: [bool; PAGE_COUNT],
}

impl FixedArena {
    pub const fn new() -> Self {
        FixedArena {
            memory: [0; PHYS_MEM_SIZE],
            bitmap: [false; PAGE_COUNT],
        }
    }
}

impl MemoryAllocator for FixedArena {
    fn mmap(&mut self, pages: u16) -> Result<Vec<u32>, AllocError> {
        if pages == 0 {
            return Err(AllocError::InvalidInput);
        }

        // Find contiguous free pages
        let mut allocated = Vec::new();
        for (idx, &used) in self.bitmap.iter().enumerate() {
            if !used {
                allocated.push((idx * PAGE_SIZE) as u32);
                if allocated.len() == pages as usize {
                    break;
                }
            }
        }

        if allocated.len() < pages as usize {
            return Err(AllocError::OutOfMemory);
        }

        // Mark pages as allocated
        for &page_base in &allocated {
            let idx = (page_base as usize) / PAGE_SIZE;
            self.bitmap[idx] = true;
        }

        Ok(allocated)
    }

    fn write(&mut self, addr: usize, data: &[u8]) -> Result<(), AllocError> {
        if addr + data.len() > PHYS_MEM_SIZE {
            return Err(AllocError::Other);
        }
        self.memory[addr..addr + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn read(&mut self, addr: usize, len: usize) -> Result<Vec<u8>, AllocError> {
        if addr + len > PHYS_MEM_SIZE {
            return Err(AllocError::Other);
        }
        Ok(self.memory[addr..addr + len].to_vec())
    }
}
```

**For QEMU with memory-mapped regions:**

```rust
// Direct access to QEMU memory region (e.g., 0x4000_0000 - 0x4010_0000)
const QEMU_MEM_BASE: usize = 0x4000_0000;

impl MemoryAllocator for QemuMemory {
    fn write(&mut self, addr: usize, data: &[u8]) -> Result<(), AllocError> {
        let phys = QEMU_MEM_BASE + addr;
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                phys as *mut u8,
                data.len()
            );
        }
        Ok(())
    }
    // ... similar for read
}
```

---

## Step 3: Implement DisplayDevice

For QEMU or embedded without a display, use UART output:

```rust
use chip8_core::device::device::{DisplayDevice, DisplayMode};
use chip8_core::proc::proc::Registers;

pub struct UartDisplay {
    // UART hardware abstraction (e.g., stm32f4xx_hal::serial)
}

impl DisplayDevice for UartDisplay {
    fn poll_input(&mut self, _capture_text: bool) {
        // Poll UART for input (optional)
    }

    fn clear_screen(&mut self) {
        // Send ANSI escape code to clear terminal
        self.uart_write(b"\x1B[2J\x1B[H");
    }

    fn draw_sprite(&mut self, regs: &mut Registers, sprite: &[u8], x: u32, y: u32) {
        // Option 1: Buffer sprites and output on present_if_due()
        // Option 2: Send ANSI codes to draw at (x, y)
        // Option 3: Ignore (headless mode)
        regs.V[0xF] = 0; // No collision detection in UART mode
    }

    fn present_if_due(&mut self) {
        // Flush UART buffer if needed
    }

    fn is_key_down(&self, _key: u8) -> bool {
        false // No keyboard in embedded
    }

    fn last_key(&self) -> Option<u8> {
        None
    }

    fn drain_text_input(&mut self) -> Vec<u8> {
        Vec::new()
    }

    fn console_write(&mut self, data: &[u8]) {
        self.uart_write(data);
    }

    fn console_backspace(&mut self) {
        self.uart_write(b"\x08 \x08"); // Backspace + space + backspace
    }

    fn set_mode(&mut self, _mode: DisplayMode) {}
    fn mode(&self) -> DisplayMode { DisplayMode::Console }
    fn refresh_hz(&self) -> u32 { 60 }
    fn set_refresh_hz(&mut self, _chip8_hz: u32, _console_hz: u32) {}
}

impl UartDisplay {
    fn uart_write(&mut self, data: &[u8]) {
        // Platform-specific UART write
        // Example for cortex-m with UART peripheral:
        // for &byte in data {
        //     while !self.uart.is_tx_empty() {}
        //     self.uart.write(byte);
        // }
    }
}
```

**For QEMU with VirtIO console:**

```rust
// Use qemu-system's VirtIO console device
const VIRTIO_CONSOLE_BASE: usize = 0x0900_0000;

impl DisplayDevice for VirtIoDisplay {
    fn console_write(&mut self, data: &[u8]) {
        unsafe {
            let console = VIRTIO_CONSOLE_BASE as *mut u8;
            for (i, &byte) in data.iter().enumerate() {
                console.add(i).write_volatile(byte);
            }
        }
    }
    // ... implement other methods
}
```

---

## Step 4: Implement InputDevice

For embedded UART input:

```rust
use chip8_core::device::device::{InputDevice, InputError};

pub struct UartInput {
    rx_buffer: Vec<u8>,
}

impl InputDevice for UartInput {
    fn push_input(&mut self, data: &[u8]) {
        self.rx_buffer.extend_from_slice(data);
    }

    fn blocking_read_line(&mut self) -> Result<(), InputError> {
        // Poll UART until newline received
        loop {
            if let Some(byte) = self.uart_read_byte() {
                self.rx_buffer.push(byte);
                if byte == b'\n' {
                    break;
                }
            }
        }
        Ok(())
    }

    fn blocking_read_byte(&mut self) -> Result<(), InputError> {
        loop {
            if let Some(byte) = self.uart_read_byte() {
                self.rx_buffer.push(byte);
                break;
            }
        }
        Ok(())
    }

    fn write_output(&mut self, data: &[u8]) -> Result<usize, InputError> {
        // Write to UART TX
        for &byte in data {
            self.uart_write_byte(byte);
        }
        Ok(data.len())
    }
}

impl UartInput {
    fn uart_read_byte(&mut self) -> Option<u8> {
        // Platform-specific: read from UART RX register
        None // Return Some(byte) when available
    }

    fn uart_write_byte(&mut self, byte: u8) {
        // Platform-specific: write to UART TX register
    }
}
```

---

## Step 5: Create Proc and Run

```rust
#![no_std]
#![no_main]

extern crate alloc;
use alloc::sync::Arc;

use chip8_core::proc::proc::{Proc, Mutex}; // Mutex is RefCell for no_std
use chip8_core::device::device::DisplayDevice;

#[entry]
fn main() -> ! {
    // Initialize allocator (e.g., embedded-alloc or custom bump allocator)
    init_heap();

    // Create device implementations
    let display = UartDisplay::new();
    let mut allocator = FixedArena::new();

    // Allocate memory for process (1 page = 4KB)
    let page_table = allocator.mmap(1).unwrap();

    // Create process
    // Note: For no_std, Proc uses Arc<Mutex<T>> where Mutex is RefCell
    let allocator_ref = Arc::new(Mutex::new(allocator));
    let mut proc = Proc::new_with_display_and_pages(
        allocator_ref.clone(),
        display,
        1
    ).unwrap();

    // Load CHIP-8 program
    let program = include_bytes!("program.ch8");
    proc.load_program_bytes(program).unwrap();

    // Main loop
    loop {
        let _ = proc.step(0, |_, _| {
            // Syscall dispatcher (simplified for embedded)
            Ok(chip8_core::syscall::syscall::SyscallOutcome::Completed)
        });
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
```

---

## Step 6: Build and Run

### For QEMU ARM

```bash
# Build for ARM Cortex-M target
cargo build --release --target thumbv7em-none-eabihf

# Convert to binary
cargo objcopy --release --target thumbv7em-none-eabihf -- -O binary chip8.bin

# Run in QEMU
qemu-system-arm \
    -machine mps2-an385 \
    -cpu cortex-m3 \
    -kernel chip8.bin \
    -nographic \
    -serial stdio
```

### For QEMU RISC-V

```bash
cargo build --release --target riscv32imac-unknown-none-elf
cargo objcopy --release -- -O binary chip8.bin

qemu-system-riscv32 \
    -machine virt \
    -kernel chip8.bin \
    -nographic \
    -serial stdio
```

---

## Memory Layout Considerations

### Typical Embedded Memory Map

```
0x0000_0000 - 0x0000_FFFF : Flash (64KB) - ROM code
0x2000_0000 - 0x2000_FFFF : SRAM (64KB) - chip8_core allocator
0x4000_0000 - 0x4FFF_FFFF : Peripherals (UART, GPIO, etc.)
```

### Linker Script Example (memory.x)

```ld
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 64K
  RAM   : ORIGIN = 0x20000000, LENGTH = 64K
}

SECTIONS
{
  .text : {
    *(.text*)
  } > FLASH

  .data : {
    *(.data*)
  } > RAM

  .bss : {
    *(.bss*)
    *(COMMON)
  } > RAM
}
```

Place `FixedArena` in the `.bss` section:

```rust
#[link_section = ".bss"]
static mut ARENA: FixedArena = FixedArena::new();
```

---

## Limitations in no_std Mode

### What's Supported
- ✅ Full CHIP-8 opcode set
- ✅ Multiple processes (requires `alloc` for Vec, Arc)
- ✅ Page table virtual memory
- ✅ DisplayDevice trait (UART, framebuffer, etc.)
- ✅ InputDevice trait (UART, GPIO)
- ✅ Stack bounds enforcement

### What's NOT Supported (Requires std)
- ❌ SharedMemory (uses Vec with heap allocation, but MemoryAllocator trait allows alternatives)
- ❌ File system (FsDevice requires std::fs)
- ❌ Threading (Arc<Mutex<T>> works but Mutex is RefCell, no actual threading)
- ❌ Kernel scheduler (requires std, but core can run single process)

### Workarounds
- **File system**: Embed ROM bytes with `include_bytes!()` or use flash memory
- **Threading**: Not needed for single CHIP-8 process
- **Scheduler**: Run single process in loop, or implement custom scheduler

---

## Example Projects

### Minimal QEMU Example

See `examples/qemu_minimal/` (to be added) for a complete working example with:
- FixedArena allocator
- UART display/input
- Single CHIP-8 process
- Memory layout

### STM32F4 Discovery Board

See `examples/stm32f4_discovery/` (to be added) for:
- HAL integration (stm32f4xx-hal)
- LCD display (ILI9341)
- UART console
- GPIO input

---

## Testing no_std Builds

Verify chip8_core compiles without std:

```bash
cargo build --no-default-features -p chip8_core
```

Expected output: `Finished dev [unoptimized + debuginfo] target(s)`

---

## Troubleshooting

### "cannot find macro `vec!` in this scope"

Ensure `extern crate alloc;` and `use alloc::vec::Vec;` are present.

### "trait bound `RefCell<T>: Sync` is not satisfied"

`Arc<RefCell<T>>` requires `T: Send`. Ensure your allocator is `Send`:

```rust
unsafe impl Send for FixedArena {}
```

### Linker Error: "undefined reference to `__aeabi_memcpy`"

Add compiler-builtins:

```toml
[dependencies]
compiler_builtins = { version = "0.1", features = ["mem"] }
```

### QEMU Hangs on Startup

Check that entry point is correct in linker script and `.cargo/config.toml`:

```toml
[target.thumbv7em-none-eabihf]
runner = "qemu-system-arm -machine mps2-an385 -cpu cortex-m3 -kernel"
```

---

## Next Steps

1. **Implement MemoryAllocator** for your target (FixedArena or memory-mapped)
2. **Implement DisplayDevice** (UART, LCD, framebuffer)
3. **Implement InputDevice** (UART, GPIO)
4. **Create minimal example** with included ROM bytes
5. **Test in QEMU** before deploying to hardware
6. **Optimize memory layout** (stack size, heap size, page count)

---

## References

- [Embedded Rust Book](https://doc.rust-lang.org/embedded-book/)
- [cortex-m Quickstart](https://github.com/rust-embedded/cortex-m-quickstart)
- [QEMU ARM Machines](https://www.qemu.org/docs/master/system/arm/mps2.html)
- [RISC-V Embedded](https://github.com/rust-embedded/riscv)

---

**Generated as part of Week 2: QEMU Portability (Phase -1)**
