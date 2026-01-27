# Code Tour: Chip-8 Runtime + Rust Concepts

This document is a guided tour of the project with an emphasis on Rust
concepts that show up in the codebase. It is intended to help you understand
both "what the runtime does" and "why the Rust looks the way it does."

---

## 1) Module Structure and `mod` Organization

Rust modules map to files and control visibility.

- `src/lib.rs` declares the public library surface:
  - `pub mod shared_memory;`
  - `pub mod proc;`
  - `pub mod kernel;`
  - etc.

- `src/main.rs` is the binary entrypoint. It pulls in modules with `mod ...`
  and uses them directly. The binary focuses on runtime wiring (root dir,
  spawning ROMs, scheduler).

Key Rust concept:
- `pub mod x` exposes module `x` to external crates or other modules.
- `mod x` only brings the module into scope for this crate.

Where to look:
- Module exports: `src/lib.rs`
- Binary wiring: `src/main.rs`

---

## 2) Types, Structs, and Ownership Boundaries

This project uses plain structs for core runtime state:

- `Proc` (in `src/proc.rs`): a single Chip-8 process.
- `Kernel` (in `src/kernel.rs`): the OS-like owner and scheduler.
- `SharedMemory` (in `src/shared_memory.rs`): the physical memory arena.

Rust concept: **ownership**
- `Kernel` owns the `HashMap<u32, ProcEntry>` for process storage.
- Each `Proc` owns its own registers, display, and page table.
- Shared memory is owned by an `Arc<Mutex<SharedMemory>>` so multiple procs can
  mutate it safely.

Example (Proc owns its fields):
```
pub struct Proc {
    pub regs: Registers,
    pub mem: Arc<Mutex<SharedMemory>>,
    pub display: DisplayWindow,
    pub page_table: Vec<u32>,
    pub vm_size: u32,
    last_timer_tick: Instant,
}
```

Why this matters in Rust:
- Ownership defines who can mutate or move data.
- The compiler enforces that only one mutable reference exists at a time.

Where to look:
- `Proc` and `Kernel` definitions in `src/proc.rs` and `src/kernel.rs`.

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
- Memory access helpers in `src/proc.rs`
- `SharedMemory` in `src/shared_memory.rs`

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
- `src/kernel.rs` for both enums and how they drive scheduling.

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
- `src/chip8_engine.rs`

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
- `src/shared_memory.rs` for allocation checks.
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
- `Proc::step` in `src/proc.rs`
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
reads.

Where to look:
- `src/kernel.rs` (`InputMode`, `sys_input_mode`, `sys_read`)
- `SYSCALLS.md` for the ABI

---

## 15) Practical Reading Guide

Suggested reading order:

1. `src/shared_memory.rs` (allocator + bounds checks)
2. `src/proc.rs` (virtual memory + stepping)
3. `src/chip8_engine.rs` (opcode dispatch)
4. `src/kernel.rs` (scheduler + syscalls)
5. `SYSCALLS.md` (ABI details)
6. `tests/syscalls.rs` (real syscall frames)

This matches the runtime stack: memory -> process -> instructions -> OS layer.

---

## 16) End-to-End Syscall Walkthrough (Annotated Trace)

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
proc.step(|id, proc| kernel.dispatch_syscall(pid, proc, id))
```

Inside `Proc::step`:
1. Polls input, ticks timers.
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
