# Code Tour: Chip-8 Runtime + Rust Concepts

This document is a guided tour of the project with an emphasis on Rust
concepts that show up in the codebase. It is intended to help you understand
both "what the runtime does" and "why the Rust looks the way it does."

---

## 1) Module Structure and `mod` Organization

Rust modules map to files and control visibility.

- `src/lib.rs` declares the public library surface for the host runtime and
  re-exports core modules where appropriate.
- The core interpreter code now lives in `chip8_core/` and is re-exported by
  `src/proc.rs`, `src/shared_memory.rs`, and `src/chip8_engine.rs` so older
  imports keep working.

- `src/main.rs` is the binary entrypoint. It pulls in modules with `mod ...`
  and uses them directly. The binary focuses on runtime wiring (root dir,
  spawning ROMs, scheduler).

Key Rust concept:
- `pub mod x` exposes module `x` to external crates or other modules.
- `mod x` only brings the module into scope for this crate.

Where to look:
- Module exports: `src/lib.rs`, `chip8_core/src/lib.rs`
- Binary wiring: `src/main.rs`

---

## 2) Types, Structs, and Ownership Boundaries

This project uses plain structs for core runtime state:

- `Proc` (in `chip8_core/src/proc.rs`, re-exported by `src/proc.rs`): a single Chip-8 process.
- `Kernel` (in `src/kernel.rs`): the OS-like owner and scheduler.
- `SharedMemory` (in `chip8_core/src/shared_memory.rs`, re-exported by `src/shared_memory.rs`):
  the physical memory arena.

Rust concept: **ownership**
- `Kernel` owns the `HashMap<u32, ProcEntry>` for process storage.
- Each `Proc` owns its own registers, display, and page table.
- Shared memory is owned by an `Arc<Mutex<SharedMemory>>` so multiple procs can
  mutate it safely.

Example (Proc owns its fields):
```
pub struct Proc<D: DisplayDevice> {
    pub regs: Registers,
    pub mem: Arc<Mutex<SharedMemory>>,
    pub display: D,
    pub page_table: Vec<u32>,
    pub vm_size: u32,
}
```

Why this matters in Rust:
- Ownership defines who can mutate or move data.
- The compiler enforces that only one mutable reference exists at a time.

Where to look:
- `Proc` definition in `chip8_core/src/proc.rs`
- `Kernel` definition in `src/kernel.rs`

---

## 3) Borrowing, `&mut`, and Why `Arc<Mutex<T>>` Appears

Rust allows many readers or one writer at a time. The runtime needs shared
mutable memory across processes, which is why you see:

- `Arc<Mutex<SharedMemory>>`

Rust concept:
- `Arc<T>` is an atomically reference-counted pointer for shared ownership.
- `Mutex<T>` enforces a single mutable access at a time.

Example:
```
let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
```

Inside `Proc::read_u8`:
- The proc locks the mutex, reads, then unlocks automatically when the guard
  is dropped.

Where to look:
- Memory access helpers in `chip8_core/src/proc.rs`
- `SharedMemory` in `chip8_core/src/shared_memory.rs`

---

## 4) Enums for Control Flow (`SyscallOutcome`, `ProcState`)

Rust enums let you model state without magic numbers.

`SyscallOutcome`:
```
pub enum SyscallOutcome {
    Completed,
    Blocked,
    Yielded,
}
```

`ProcState`:
```
pub enum ProcState {
    Running,
    Blocked,
    Exited,
}
```

Rust concept:
- Enums are often used instead of boolean flags or integer codes.
- They are exhaustively matched, so the compiler ensures you handle all cases.

Where to look:
- `chip8_core/src/syscall.rs` for `SyscallOutcome` (re-exported by `src/kernel.rs`)
- `src/kernel.rs` for `ProcState` and scheduling logic.

---

## 5) Pattern Matching and Opcode Dispatch

Opcode dispatch relies heavily on `match`:

```
match opcode {
    0x0 => opcode_0x0(...),
    0x1 => opcode_0x1(...),
    ...
    _ => panic!("Unknown opcode"),
}
```

Rust concept:
- `match` is an expression (returns a value).
- The compiler checks that all cases are covered.

In `opcode_0x0`, the dispatcher matches:
- `0x00E0` (CLS)
- `0x00EE` (RET)
- otherwise treat `0nnn` as a syscall if in range.

Where to look:
- `chip8_core/src/chip8_engine.rs` (re-exported by `src/chip8_engine.rs`)

---

## 6) Generics and Trait Bounds (Syscall Registration)

The syscall table accepts any handler type that satisfies a callable signature:

```
pub fn register<H>(&mut self, id: u16, handler: H) -> Result<(), Error>
where
    H: Fn(&mut Kernel, u32, &mut Proc) -> SyscallOutcome + Send + Sync + 'static,
```

Rust concept:
- `H` is generic. The compiler "fills in" the concrete type based on usage.
- Trait bounds constrain what `H` must support.
  - `Fn(...)`: callable like a function.
  - `Send + Sync`: safe across threads.
  - `'static`: no borrowed references that could outlive their owners.

Why this matters:
- You can pass a function or closure directly.
- The table stores them as `Arc<dyn Fn...>` so many different handler types can
  live together in one map.

Where to look:
- `SyscallTable::register` in `src/kernel.rs`.

---

## 7) Dynamic Dispatch (`dyn Fn`) + `Arc`

The syscall table stores handlers as:

```
Arc<dyn Fn(&mut Kernel, u32, &mut Proc) -> SyscallOutcome + Send + Sync>
```

Rust concept:
- `dyn Trait` is a trait object, which allows **dynamic dispatch**.
- `Arc` allows cheap cloning and shared ownership.

Why this matters:
- The syscall map can hold different handler types under a single signature.
- You can look up and call handlers uniformly by ID.

Where to look:
- `SyscallTable` in `src/kernel.rs`.

---

## 8) `Result`, `Error`, and Early Returns

Most fallible operations return `Result<T, Error>`. The code uses early returns
to keep error paths clear:

```
if !(0x0100..0x0200).contains(&id) {
    return Err(Error::new(ErrorKind::InvalidInput, "syscall id out of range"));
}
```

Rust concept:
- `Result<T, E>` is the standard error type.
- `?` propagates errors; explicit `return Err(...)` is used when needed.

Where to look:
- `chip8_core/src/shared_memory.rs` for allocation checks.
- `src/kernel.rs` for syscall validation and IO errors.

---

## 9) Closures as Dependencies (Kernel -> Proc -> Dispatcher)

The kernel provides a syscall dispatcher to the proc’s step function:

```
proc.step(|id, proc| self.dispatch_syscall(pid, proc, id))
```

Rust concept:
- Functions can accept closures (`FnMut`), which can capture variables like `pid`.
- This avoids global state and keeps syscall routing in the kernel.

Why this matters:
- `Proc` stays generic and reusable.
- The kernel controls policy (blocking/yielding) without embedding it in the proc.

Where to look:
- `Proc::step` in `chip8_core/src/proc.rs`
- `Kernel::run` and `Kernel::dispatch_syscall` in `src/kernel.rs`

---

## 10) Ranges and Iterators

Rust often uses ranges to express bounds:

```
if !(0x0100..0x0200).contains(&id) { ... }
```

Rust concept:
- `0x0100..0x0200` is a half-open range (end excluded).
- `.contains` is a method on the range type.

Where to look:
- Syscall ID validation in `src/kernel.rs`.

---

## 11) Scheduling and State Mutation in Rust

The scheduler needs to mutate proc state without violating borrow rules.

Technique used:
1. Remove the proc entry from the map.
2. Mutate it.
3. Insert it back.

This avoids mutable and immutable borrows of the map at the same time.

Where to look:
- `Kernel::run_proc_until_yield_or_block` in `src/kernel.rs`.

---

## 12) Testing Patterns in Rust

Tests in this repo are integration tests in `tests/`:

- `tests/opcode_semantics.rs` for opcode behavior.
- `tests/syscalls.rs` for syscall behavior.

Rust concepts shown:
- `#[test]` functions are picked up by the test harness.
- `std::env::set_var` is `unsafe` on this toolchain, so tests wrap it in
  `unsafe { ... }` with a `Once` guard.
- Temporary directories are built manually using `std::env::temp_dir`.

Where to look:
- `tests/syscalls.rs` for syscall frames and kernel stepping.

---

## 13) Headless Mode and Environment Configuration

To allow testing without a GUI window:

- `DisplayWindow::from_env()` checks `CHIP8_HEADLESS`.
- Syscalls and the CLI use `from_env()` for display creation.

Rust concept:
- `std::env::var` returns `Result<String, VarError>`; the code treats "present"
  as a boolean flag.

Where to look:
- `src/display.rs`
- `src/kernel.rs` (spawn)
- `src/main.rs` (root proc creation)

---

## 14) Input Modes (Line vs Byte)

`sys_read` can behave in two ways, per-process:

- **Line mode**: the kernel waits for a newline and delivers bytes up to the
  newline (or the requested length).
- **Byte mode**: the kernel delivers whatever bytes are available immediately.

Switching is done via the `input_mode` syscall (`0x0112`). This lets CLI ROMs
opt into line-oriented input without preventing other ROMs from using byte-precise
reads. Output/input routing is controlled separately via `console_mode`
(`0x0113`), which allows a ROM to opt into the display-backed text console.

Where to look:
- `src/kernel.rs` (`InputMode`, `ConsoleMode`, `sys_input_mode`, `sys_console_mode`, `sys_read`)
- `SYSCALLS.md` for the ABI

---

## 15) Filesystem Syscalls and Root Validation

The runtime exposes a host-backed filesystem to ROMs via `fs_list`, `fs_open`,
`fs_read`, and `fs_close`. All paths are resolved **relative to the kernel
root directory**. Absolute paths and `..` are rejected, and the root tree is
validated at startup to enforce limits (max filename length, max entries per
dir, max file size).

Rust concepts in play:
- `std::fs::read_dir` for directory iteration.
- `Path` + `Component` for safe path normalization.
- `std::fs::File` stored in a kernel-owned FD table keyed by pid.

Where to look:
- `src/kernel.rs` (`sys_fs_list`, `sys_fs_open`, `sys_fs_read`, `sys_fs_close`)
- `src/kernel.rs` (per-proc FD table)
- `SYSCALLS.md` (ABI + record layout)

---

## 16) Practical Reading Guide

Suggested reading order:

1. `chip8_core/src/shared_memory.rs` (allocator + bounds checks)
2. `chip8_core/src/proc.rs` (virtual memory + stepping)
3. `chip8_core/src/chip8_engine.rs` (opcode dispatch)
4. `src/kernel.rs` (scheduler + syscalls)
5. `SYSCALLS.md` (ABI details)
6. `tests/syscalls.rs` (real syscall frames)

This matches the runtime stack: memory -> process -> instructions -> OS layer.

---

## 17) End-to-End Syscall Walkthrough (Annotated Trace)

This is a single, concrete walkthrough of a syscall from the moment a Chip-8
program executes the instruction to the moment the scheduler resumes the next
instruction. We'll use **`SYS write` (0x0110)** because it exercises the syscall
frame, memory reads, and scheduler flow without blocking.

Scenario: a program wants to print "hello".

### 15.1 Initial State (Before the Syscall)

Assume a 1-page VM (4KB). The program has placed the string at `0x0320` and a
syscall frame at `0x0300`. Registers and memory look like this:

Registers:
```
PC = 0x0200   ; next instruction
I  = 0x0300   ; syscall frame pointer
V0 = 0x00     ; unused before call
VF = 0x00     ; clear (no error)
SP = 0x1000   ; stack top (downward-growing)
```

Memory (virtual addresses shown):
```
0x0300: 0x05        ; frame length = 1 + (2 args * 2 bytes)
0x0301: 0x03 0x20   ; arg0 = 0x0320 (buffer pointer)
0x0303: 0x00 0x05   ; arg1 = 5      (length)

0x0320: 0x68 0x65 0x6C 0x6C 0x6F  ; "hello"
```

Instruction at PC:
```
0x0200: 0x0110  ; SYS write
```

### 15.2 Fetch → Decode → Dispatch

The kernel calls:
```
proc.step(ticks, |id, proc| kernel.dispatch_syscall(pid, proc, id))
```

Inside `Proc::step`:
1. Polls input, applies the tick count provided by the kernel.
2. Reads two bytes from virtual memory at `PC` (0x0200).
3. Builds the instruction `0x0110`.
4. Extracts opcode nibble `0x0`.
5. Calls `opcode_0x0` with the dispatcher closure.

In `opcode_0x0`:
- `nnn = 0x110`
- It's in `0x0100..0x01FF`, so it invokes the dispatcher:
  ```
  dispatch_syscall(0x0110, proc)
  ```

### 15.3 Kernel Dispatch

`Kernel::dispatch_syscall(pid, proc, id)`:
1. Looks up the handler in the syscall table:
   - `0x0110` → `sys_write`
2. Calls the handler:
   ```
   sys_write(kernel, pid, proc)
   ```

### 15.4 Syscall Handler Reads the Frame

`sys_write` resolves its arguments using the syscall frame:

```
arg0 = Kernel::syscall_arg(proc, 0)
arg1 = Kernel::syscall_arg(proc, 1)
```

`Kernel::syscall_arg`:
- Reads `I` from the proc (`0x0300`).
- Reads `frame_len` at `I + 0` (`0x05`).
- Calculates the argument offset:
  - arg0 → offset 1
  - arg1 → offset 3
- Reads 16-bit big-endian values:
  - arg0 = 0x0320
  - arg1 = 0x0005

Then `sys_write` reads the buffer from proc memory:
```
proc.read_bytes(0x0320, 5) -> [0x68,0x65,0x6C,0x6C,0x6F]
```

### 15.5 Host I/O Side Effects

The handler writes the bytes to stdout:
```
stdout.write_all(b"hello")
```

On success it sets:
```
V0 = 5    ; bytes written (low 8 bits)
VF = 0    ; no error
```

It returns:
```
SyscallOutcome::Completed
```

### 15.6 PC Advancement + Scheduler Result

Back in `opcode_0x0`:
- Because the syscall completed, it advances:
  ```
  PC += 2
  ```
Now:
```
PC = 0x0202
```

`Proc::step` returns `SyscallOutcome::Completed` to the kernel.

The kernel:
- Applies any pending state transitions (none here).
- Keeps the process runnable.
- Moves on to the next process or the next instruction, depending on the scheduler.

### 15.7 Final State (After the Syscall)

Registers:
```
PC = 0x0202   ; advanced to next instruction
I  = 0x0300   ; unchanged
V0 = 0x05     ; bytes written
VF = 0x00     ; success
SP = 0x1000   ; unchanged
```

Memory:
```
frame + buffer unchanged
```

Console output:
```
hello
```

### 15.8 Notes on Blocking Syscalls

If this had been `SYS read (0x0111)` and no input was available:
- The handler would return `SyscallOutcome::Blocked`.
- The dispatcher would still advance `PC` by 2.
- The kernel would mark the proc as `Blocked` and switch to another runnable proc.
- When input arrives, the kernel writes into the buffer and resumes the proc,
  which continues **after** the syscall instruction (at the already-advanced PC).

This is why the syscall instruction is not re-executed after unblocking.

---

## 16) CLI ROM (c8asm) — End-to-End Example in Chip-8

The runtime is now complemented by a **c8asm-based CLI ROM** under `roms/cli/`.
This ROM is intentionally small but fully functional, and it serves two roles:

1) A real user-facing shell for the runtime.
2) A living example of how to use the syscall ABI from Chip-8.

The key design goal is readability: anyone can open the ROM and understand how
to build syscall frames, call the kernel, and parse input without hunting
through host code.

The CLI ROM opts into the **display-backed console** (`console_mode = 1`), so
all input/output occurs inside the Chip-8 window using an 80x40 character grid
(640x320 logical pixels).

### 16.1 File Layout

```
roms/cli/
  cli.c8s         # main program + command dispatch
  lib/sys.c8s     # syscall frame builders + wrappers
  lib/data.c8s    # fixed buffers + constant strings
  build.sh        # concatenates + assembles into build/cli.ch8
```

The build step concatenates the three source files into one `.c8s` source:

```
roms/cli/build/cli_combined.c8s
```

The build script then assembles it into a `.ch8` ROM using the in-repo `c8asm`
tool. The runtime uses this ROM like any other program.

### 16.2 Why a Helper Library?

Chip-8 has no native notion of syscalls. In this runtime, syscalls are invoked
via `0nnn` where `nnn` is the syscall ID. The kernel expects a **frame** pointed
to by `I`:

```
I + 0: length (bytes, including this byte)
I + 1: arg0_hi
I + 2: arg0_lo
I + 3: arg1_hi
I + 4: arg1_lo
I + 5: arg2_hi
I + 6: arg2_lo
I + 7: arg3_hi
I + 8: arg3_lo
```

Hand-writing that frame in every ROM would be error-prone, so `lib/sys.c8s`
provides:

- **Frame builders** (`frame1`..`frame4`) that write a frame into a fixed buffer.
- **Syscall wrappers** (`sys_write`, `sys_read`, `sys_fs_list`, ...) that emit
  the `0nnn` trap after the frame is constructed.

This isolates the ABI in one place, keeps the ROM readable, and ensures a
future ROM can re-use the same library with minimal changes.

### 16.3 Fixed Addresses as a Design Tool

Chip-8 lacks an instruction to load a full 16-bit pointer into `I` using
registers. To keep pointer arithmetic simple, the ROM uses **fixed buffer
addresses** with carefully chosen low bytes:

- `LINE_BUF` lives at `0x800`, so `0x800 + offset` never carries into the high
  byte for an 80-byte buffer.
- `DIR_BUF` lives at `0x900`, so each directory record offset fits in one byte.

This lets the CLI build pointers by setting a single high byte and using the
low byte as an offset. For example:

```
arg0_hi = 0x08
arg0_lo = tok_offset
```

The same strategy is used for directory entries and file buffers.

### 16.4 Command Dispatch (Tokenization)

The CLI parses a single line into two tokens:

1) `tok1` — the command (help/ls/run/cat/exit)
2) `tok2` — optional argument (ROM name or file name)

This is done by scanning `LINE_BUF` byte-by-byte:

1) Skip leading spaces.
2) Mark `tok1` start and length until a space.
3) Skip spaces.
4) Mark `tok2` start and length until a space.

The offsets are stored in registers (`vB..vE`) and later reused to build syscall
arguments without copying the string.

### 16.5 Syscall Usage Patterns

Each command is essentially a small syscall sequence:

#### `help`
- `sys_write(HELP_TEXT, len)`

#### `ls`
- `sys_fs_list("", 0, DIR_BUF, 4)`
- Iterate over returned records, print each name.
- Append `/` for directories.

#### `run <rom>`
- `sys_spawn(tok2_ptr, tok2_len, pages=1)`
- `sys_wait(pid)` to block until it exits.

#### `cat <file>`
- `sys_fs_open(tok2_ptr, tok2_len, flags=0)`
- Loop `sys_fs_read(fd, FILE_BUF, 0x40)` and `sys_write(FILE_BUF, n)`
- `sys_fs_close(fd)`

#### `exit`
- `sys_exit(0)` then halt in-place until the scheduler removes the process.

### 16.6 Why This Matters

The CLI ROM is intentionally small but complete:

- It proves the syscall ABI works end-to-end in a real Chip-8 program.
- It demonstrates a reusable pattern for **ROM-side libraries**.
- It documents the practical constraints of Chip-8 (no pointer load, tiny RAM)
  and shows how to design around them cleanly.

For an even deeper walkthrough of the ROM itself (memory map, constants, and
string tables), see `roms/cli/README.md`.
