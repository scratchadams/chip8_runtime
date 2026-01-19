# Chip-8 Runtime Architecture

This document describes the current structure of the Chip-8 runtime, how data
flows between modules, the opcode dispatch pattern, and where the design can be
extended. It is intentionally verbose so new contributors can orient quickly.

---

## 1) High-Level Overview

The project is a Chip-8 interpreter implemented as a small runtime that:

- Creates a shared memory arena for multiple "processes" (Chip-8 programs).
- Spawns one or more Proc instances, each with a private display and one or
  more per-process virtual pages.
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
with its own registers, display, and a page table that maps virtual pages into
shared physical memory.

```
Proc
├── regs: Registers
│   ├── V[16]  : general registers V0..VF
│   ├── I      : index register
│   ├── PC     : program counter (virtual, per-process)
│   ├── SP     : stack pointer (virtual, per-process)
│   ├── DT/ST  : delay/sound timers (ticked at ~60Hz in step)
├── mem: &mut Arc<Mutex<SharedMemory>>
├── display: DisplayWindow
├── page_table: Vec<u32>   (physical bases per virtual page)
├── vm_size: u32           (virtual size in bytes)
```

Key invariants:

- `PC`, `I`, and `SP` are **virtual addresses** translated via the page table.

### 3.2 SharedMemory

`SharedMemory` models a large physical memory array plus a bitmap allocator.
Each process requests N pages; `mmap()` returns the physical base of each page.

```
SharedMemory
├── phys_mem: Vec<u8>        # 1MB physical memory
└── phys_bitmap: Vec<bool>   # page allocator bitmap (1 entry/page)
```

The allocator is intentionally simple and currently does not free pages.
Virtual-to-physical translation is handled by `Proc::translate`, which maps
virtual pages to physical bases via the per-proc page table.

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

Tests construct a headless `DisplayWindow` instance directly, which enables
opcode tests to run without GUI dependencies.

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
2. Fetches two bytes at virtual `PC` using `Proc::read_u8` (translation).
3. Combines them into a 16-bit instruction (big-endian).
4. Extracts the opcode (top nibble) and dispatches to the correct handler.

```
loop:
  poll_input()
  instr = mem[translate(PC)] << 8 | mem[translate(PC+1)]
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

The runtime uses **page-relative virtual addresses** for each process. Each
Proc owns a simple, single-level page table that maps virtual pages to physical
page bases:

```
vpage   = vaddr / PAGE_SIZE
offset  = vaddr % PAGE_SIZE
phys    = page_table[vpage] + offset
```

So, if a program uses `I = 0x1300`, `PAGE_SIZE = 0x1000`, and
`page_table[1] = 0x6000`, the physical memory location is `0x6300`.

`I` is the CHIP-8 **index register** and is explicitly meant to hold addresses
for memory operations (`Dxyn`, `Fx33`, `Fx55`, `Fx65`, `Fx29`, etc.). Translation
does not change the meaning of `I`; it only changes where that virtual address
lands in shared physical memory.

### 6.1 ROM Loading

`Proc::load_program()` loads into **virtual addresses**:

```
0x000 .. 0x050  : font sprites (80 bytes)
0x200 ..        : program text
```

This matches standard Chip-8 memory layout (program entry at 0x200).

### 6.2 Stack

`SP` is a virtual address. The current implementation stores a two-byte return
address at `SP` (translated via `Proc::write_u8`) and increments `SP` by 2 on
call.

```
CALL nnn:
  SP += 2
  mem[SP]   = high(PC+2)
  mem[SP+1] = low(PC+2)
  PC = nnn

RET:
  PC = mem[SP] << 8 | mem[SP+1]
  SP -= 2
```

This is internally consistent with the "handlers advance PC" design.

### 6.3 Classic vs Extended Addressing

Classic CHIP-8 programs assume a **4KB address space** (0x000..0xFFF). Because
most opcodes encode only a **12-bit address** (`nnn`), classic ROMs cannot
directly refer to addresses above 0x0FFF.

- **Classic mode (default)**  
  - `vm_size = 0x1000` (one page).
  - `PC`, `I`, and `SP` must stay within 0x000..0xFFF.
  - All addressing remains spec-compliant.

- **Extended mode (multi-page)**  
  - `vm_size = N * 0x1000` (multiple pages).
  - `PC`, `I`, and `SP` may reference addresses above 0x0FFF.
  - Standard opcodes still only encode 12-bit immediates, so **extended
    programs need a new mechanism** to set larger addresses (e.g., a syscall
    frame, or a new opcode that loads a 16-bit address into `I`/`PC`).
  - This approach preserves classic ROM behavior while enabling larger stacks
    and data regions for extended ROMs.

### 6.4 Visual: Virtual to Physical Translation

```
Virtual address (vaddr)
+---------------------------+
|  vpage  |    offset       |
| vaddr/PS|  vaddr%PS       |
+---------+-----------------+
     |                 |
     | lookup          | add
     v                 v
page_table[vpage]   offset
     |                 |
     +--------+--------+
              v
        phys = base + offset
```

### 6.5 Visual: Example Page Mapping

```
Proc virtual space (2 pages)           Shared physical memory
0x0000 .. 0x0FFF  ------------------>  page_table[0] = 0x4000..0x4FFF
0x1000 .. 0x1FFF  ------------------>  page_table[1] = 0x9000..0x9FFF

Example:
  I = 0x1300
  vpage = 0x1300 / 0x1000 = 1
  offset = 0x300
  phys = 0x9000 + 0x300 = 0x9300
```

---

## 7) Rendering and Input

### 7.1 Display Rendering

Chip-8 uses XOR drawing and collision detection. The opcode handler:

- Reads sprite bytes from `mem[I..I+n]` via `Proc::read_u8`.
- Passes those bytes to `DisplayWindow::draw_sprite`.
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
        |  page_table      |        +------------------+
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
PC -> [mem[translate(PC)], mem[translate(PC+1)]] -> instruction
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
vpage = addr / PAGE_SIZE
         |
         v
physical base = page_table[vpage]
         |
         v
phys = physical base + (addr % PAGE_SIZE)
         |
         v
SharedMemory.phys_mem[phys]
```

---

## 10) Notable Patterns and Design Choices

- **Opcode grouping by first nibble** keeps decode logic compact and readable.
- **Macro-based field extraction** centralizes bit handling and reduces bugs.
- **Per-process display** allows multiple concurrent Proc instances with their
  own windows or test-only headless buffers.
- **SharedMemory + page_table** is a small “virtualization” layer; it enables
  separate address spaces, and `Proc::translate` centralizes the mapping logic.

---

## 11) Suggested Next Steps / Improvements

1. **Timer accuracy**  
   Timers now tick at ~60Hz inside `Proc::step`, but the cadence depends on how
   often `step()` is called. If you add a scheduler or throttling, consider
   decoupling the timer tick from instruction rate.

2. **Configurable ROM loading**  
   `main.rs` hard-codes paths like `/root/rust/chip8/ibm.ch8`. Add CLI args or
   a config file to select ROMs and run modes.

3. **Opcode strictness and invalid opcodes**  
   Some handlers accept any `0x5xy?` or `0x9xy?` without verifying the low nibble.
   Decide whether to enforce exact opcode shapes and log invalid forms.

4. **Memory allocator lifecycle**  
   The allocator is `Vec<bool>` and supports multi-page allocations, but pages
   are never freed. If you add process teardown, implement `munmap()` and
   consider fragmentation/compaction.

5. **Display and input abstraction**  
   If you plan to support other frontends (SDL, web), consider a trait or
   interface for display/input backends.

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
