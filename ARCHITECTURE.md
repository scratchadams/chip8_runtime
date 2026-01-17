# Chip-8 Runtime Architecture

This document describes the current structure of the Chip-8 runtime, how data
flows between modules, the opcode dispatch pattern, and where the design can be
extended. It is intentionally verbose so new contributors can orient quickly.

---

## 1) High-Level Overview

The project is a Chip-8 interpreter implemented as a small runtime that:

- Creates a shared memory arena for multiple "processes" (Chip-8 programs).
- Spawns one or more Proc instances, each with a private display and a
  per-process memory page.
- Executes a fetch-decode-execute loop that dispatches to opcode handlers.
- Renders sprites to a scaled pixel buffer using minifb.
- Provides test facilities for headless execution and single-step inspection.

In short, each Proc models a single Chip-8 virtual machine that runs on top of
the shared memory allocator.

---

## 2) Module Map

```
chip8_runtime/
├── src/
│   ├── main.rs            # binary entrypoint, spawns Proc threads
│   ├── lib.rs             # exports modules for tests/integration use
│   ├── proc.rs            # Proc, Registers, and runtime loop
│   ├── chip8_engine.rs    # opcode handlers and opcode field macros
│   ├── display.rs         # DisplayWindow and sprite rendering
│   └── shared_memory.rs   # SharedMemory allocator + read/write helpers
└── tests/
    └── opcode_semantics.rs # unit-style opcode tests (headless)
```

---

## 3) Core Data Structures

### 3.1 Proc and Registers

`Proc` is the primary execution unit. It represents a single Chip-8 process
with its own registers, display, and a base address into shared memory.

```
Proc
├── proc_id: u32
├── regs: Registers
│   ├── V[16]  : general registers V0..VF
│   ├── I      : index register
│   ├── PC     : program counter (virtual, per-process)
│   ├── SP     : stack pointer (virtual, per-process)
│   ├── DT/ST  : delay/sound timers (currently manual)
├── mem: &mut Arc<Mutex<SharedMemory>>
├── display: DisplayWindow
└── base_addr: u32  (physical page base for this process)
```

Key invariants:

- `PC` and `I` are treated as **page-relative** addresses.
- `base_addr` is the physical offset for this process's 4KB page.
- `SP` is also page-relative and used to store return addresses in memory.

### 3.2 SharedMemory

`SharedMemory` models a large physical memory array plus a bitmap allocator.
Each process requests a page; `base_addr` is the start of that page.

```
SharedMemory
├── phys_mem: Vec<u8>        # 1MB physical memory
├── page_table_entries: Vec<Vec<u32>>   # simple page table
└── phys_bitmap: u128        # page allocator bitmap (1 bit/page)
```

The allocator is intentionally simple and currently supports only one page per
process. Virtual-to-physical translation is done manually in code by adding
`base_addr`.

### 3.3 DisplayWindow

`DisplayWindow` stores a pixel buffer and (optionally) a live window handle.
It renders a 64x32 logical Chip-8 display scaled by `SCALE` (currently 10).

```
DisplayWindow
├── window: Option<minifb::Window>
├── buf: Vec<u32>            # WIDTH * HEIGHT pixels
├── key_down: [bool; 16]     # full keypad state
├── last_key: Option<u8>     # one key currently held
└── key_state: u8            # compatibility alias (0xFF = none)
```

`new_headless()` builds a display without a window; this enables tests to run
without GUI dependencies.

---

## 4) Execution Flow

### 4.1 Main Thread and Proc Spawning

`src/main.rs` shows the current usage pattern: a shared memory arena is created
and multiple threads each spawn a `Proc` with its own display. Each thread loads
a program and calls `run_program()` in an infinite loop.

```
main() ──> SharedMemory::new() ──┐
                                ├─ spawn thread ─> Proc::new() ─> load_program() ─> run_program()
                                └─ spawn thread ─> Proc::new() ─> load_program() ─> run_program()
```

Important: the ROM paths are currently hard-coded and should be made
configurable for real usage.

### 4.2 Fetch-Decode-Execute Loop

`Proc::run_program()` repeatedly calls `step()`. The step function:

1. Polls input (`DisplayWindow::poll_input`).
2. Fetches two bytes at `base_addr + PC`.
3. Combines them into a 16-bit instruction (big-endian).
4. Extracts the opcode (top nibble) and dispatches to the correct handler.

```
loop:
  poll_input()
  instr = mem[base+PC] << 8 | mem[base+PC+1]
  opcode = instr >> 12
  dispatch(opcode, instr)
```

Each handler is responsible for **advancing PC**. The dispatcher does not
auto-increment.

---

## 5) Opcode Dispatch Pattern

`chip8_engine.rs` organizes opcode handlers by their top nibble. This is a
common pattern in Chip-8 interpreters, since opcodes are grouped by their
leading 4 bits.

### 5.1 Field Extraction Macros

Macros are used to extract standard fields from the 16-bit instruction:

```
extract_nnn!(inst)  => lowest 12 bits (address)
extract_x!(inst)    => X register index (bits 8..11)
extract_y!(inst)    => Y register index (bits 4..7)
extract_kk!(inst)   => immediate 8-bit value
extract_z!(inst)    => lowest 4 bits (sub-opcode)
```

This keeps per-opcode logic short and reduces bit manipulation mistakes.

### 5.2 Example: Arithmetic Opcodes

`opcode_0x8` is a good example of a "sub-dispatch" pattern. It uses `extract_z`
to choose among multiple arithmetic/logical operations:

```
8xy0: Vx = Vy
8xy1: Vx |= Vy
8xy2: Vx &= Vy
8xy3: Vx ^= Vy
8xy4: Vx += Vy; VF = carry
8xy5: Vx -= Vy; VF = NOT borrow
8xy6: Vx >>= 1; VF = LSB of Vx
8xy7: Vx = Vy - Vx; VF = NOT borrow
8xyE: Vx <<= 1; VF = MSB of Vx
```

This is a direct transcription of the Columbia spec referenced for the
project.

---

## 6) Memory Model and Addressing

The runtime uses **page-relative virtual addresses** for each process:

```
virtual address (PC/I/SP) -> base_addr + virtual offset -> phys_mem[]
```

So, if a program uses `I = 0x300` and the process's `base_addr = 0x4000`, the
physical memory location is `0x4300`.

### 6.1 ROM Loading

`Proc::load_program()` loads:

```
base_addr + 0x000 .. 0x050  : font sprites (80 bytes)
base_addr + 0x200 ..        : program text
```

This matches standard Chip-8 memory layout (program entry at 0x200).

### 6.2 Stack

`SP` is page-relative. The current implementation stores a two-byte return
address at `base_addr + SP` and increments `SP` by 2 on call.

```
CALL nnn:
  SP += 2
  mem[base+SP]   = high(PC+2)
  mem[base+SP+1] = low(PC+2)
  PC = nnn

RET:
  PC = mem[base+SP] << 8 | mem[base+SP+1]
  SP -= 2
```

This is internally consistent with the "handlers advance PC" design.

---

## 7) Rendering and Input

### 7.1 Display Rendering

Chip-8 uses XOR drawing and collision detection. The renderer:

- Reads sprite bytes from `mem[I..I+n]`.
- XORs each bit with the current buffer.
- Sets `VF = 1` if any pixels are erased (collision).
- Wraps X/Y when sprite extends past the screen.

### 7.2 Key Input

`DisplayWindow::poll_input()` captures the current pressed state for all 16
keys and tracks a single `last_key` for `Fx0A` blocking behavior. This matches
the Chip-8 expectation that opcodes can query whether a specific key is down.

Mapping (classic Chip-8 keyboard layout):

```
1 2 3 C         -> 1 2 3 4
4 5 6 D         -> Q W E R
7 8 9 E         -> A S D F
A 0 B F         -> Z X C V
```

---

## 8) Testing Strategy


1. **Opcode semantics tests** (`tests/opcode_semantics.rs`):
   - Headless, single-opcode execution.
   - Tests edge cases like borrow/carry, shift flags, BCD writes,
     call/return stack, draw collision, and key waits.


---

## 9) ASCII Architecture Diagrams

### 9.1 Module Interaction

```
        +------------------+
        |  main.rs         |
        |  spawns Proc(s)  |
        +---------+--------+
                  |
                  v
        +------------------+        +------------------+
        |  Proc            |        |  DisplayWindow   |
        |  regs, I/PC/SP   |<------>|  buf, input      |
        |  base_addr       |        +------------------+
        |  mem (Shared)    |
        +---------+--------+
                  |
                  v
        +------------------+
        | SharedMemory     |
        | phys_mem + bitmap|
        +------------------+
```

### 9.2 Fetch-Decode-Execute Loop

```
PC -> [mem[base+PC], mem[base+PC+1]] -> instruction
             |
             v
     opcode = instruction >> 12
             |
             v
    dispatch to opcode_0xN handler
             |
             v
   handler updates registers, memory, display, PC
```

### 9.3 Address Translation

```
virtual addr (I/PC/SP)
         |
         v
physical addr = base_addr + virtual addr
         |
         v
SharedMemory.phys_mem[physical addr]
```

---

## 10) Notable Patterns and Design Choices

- **Opcode grouping by first nibble** keeps decode logic compact and readable.
- **Macro-based field extraction** centralizes bit handling and reduces bugs.
- **Per-process display** allows multiple concurrent Proc instances with their
  own windows (or headless buffers).
- **SharedMemory + base_addr** is a small “virtualization” layer; it enables
  separate address spaces without a complex MMU.

---

## 11) Suggested Next Steps / Improvements

1. **Timer ticking**  
   Delay and sound timers (`DT`/`ST`) are updated only when opcodes write them.
   A 60Hz tick should decrement them to be spec‑accurate.

2. **Configurable ROM loading**  
   `main.rs` hard-codes paths like `/root/rust/chip8/ibm.ch8`. Add CLI args or
   a config file to select ROMs and run modes.

3. **Opcode strictness and invalid opcodes**  
   Some handlers accept any `0x5xy?` or `0x9xy?` without verifying the low nibble.
   Decide whether to enforce exact opcode shapes and log invalid forms.

4. **Memory allocator bounds**  
   `phys_bitmap` is `u128` but `PHYS_MEM_SIZE / PAGE_SIZE` is 256 pages. If you
   plan to allocate >128 pages, expand the bitmap or replace with `Vec<bool>`.

5. **Display and input abstraction**  
   The new `new_with_display` and `new_headless` hooks are good. If you plan to
   support other frontends (SDL, web), consider a trait or interface for
   display/input backends.

6. **Super-CHIP/XO-CHIP extensions**  
   The Timendus suite includes scrolling and high‑res tests. If you want to pass
   those, add scrolling opcodes, 128x64 mode, and associated quirks.

7. **Test coverage for timing and sound**  
   Tests now cover opcode semantics thoroughly. Add timer-tick tests once the
   60Hz mechanism is implemented.

---

## 12) Summary

This runtime is structured around a clear separation of concerns:

- `Proc` handles CPU state and execution.
- `chip8_engine` handles opcode semantics.
- `DisplayWindow` handles graphics and input.
- `SharedMemory` handles per-process memory allocation.

The headless test path and opcode semantics suite make the core CPU logic
observable and reliable. The next major step is implementing timing behavior
and optional CHIP-8 extensions if you want to pass the full suite of tests.
