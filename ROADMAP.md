# Chip-8 OS Extension Roadmap

This roadmap outlines a realistic path from the current Chip-8 runtime to an
"extended Chip-8" environment that can behave like a small OS: a CLI that can
launch other Chip-8 programs, a minimal filesystem, and multi-process support.

The guiding principle is to preserve classic Chip-8 compatibility while adding
explicit extension points (new opcodes or trap/syscall semantics) for OS-like
features.

---

## Guiding Principles

- Keep normal Chip-8 ROMs running unchanged.
- Add explicit extension opcodes or a syscall trap to avoid ambiguity.
- Start with cooperative scheduling; add preemption only when needed.
- Favor a stable ABI (register-based conventions) for syscalls.
- Grow memory and features incrementally to avoid breaking tests.

---

## Phase 0: Define the Extended Chip-8 Contract (1-2 weeks)

Before writing new runtime features, define the interface.

### Decisions to lock in

1. **Syscall invocation mechanism**
   - Option A: Reserve a new opcode group (e.g., `0xF?` with special subcodes).
   - Option B: Define a TRAP opcode (e.g., `0x000F` or `0x00F0`).
   - Option C: Reuse `0x0nnn` or unused legacy opcodes for host calls.
   - Decision: reserve `0x0100..0x01FF` for syscall dispatch (rest of 0nnn reserved).

2. **Register ABI**
   - Example ABI:
     - `V0` = syscall ID
     - `V1..Vx` = arguments
     - `V0` = return value
     - `VF` = error flag (0 = ok, 1 = error)

3. **Memory model**
   - Decide default page counts per Proc (multi-page is now supported).
   - Decide if a shared page exists for IPC or filesystem buffers.

4. **Process model**
   - Cooperative (yield opcode) vs preemptive (timer interrupt).
   - Start with cooperative scheduling for simplicity.

### Deliverable

- Update `EXTENSION.md` to reflect the current runtime foundation
  (single-level paging, translation helpers, allocator behavior).
- Lock a syscall ABI and opcode encoding for the first syscall milestone.

---

## Phase 1: Syscall Dispatcher + Minimal ABI Tests (2-4 weeks)

Goal: syscall support with tests, before building the CLI ROM.

### Required runtime changes

- Syscall dispatcher with a stable ABI (likely `0nnn`).
- Runtime syscall registry or table (host-side mapping from ID to handler).
- A kernel/runtime struct to own process bookkeeping (since `ProcessTable` was removed).
- Process lifecycle:
  - `spawn(rom_name) -> pid`
  - `exit(code)`
  - `wait(pid) -> exit_code`
  - `yield()`

### Example syscall IDs

```
SYS 0x01: spawn
  in:  V1 = ptr to string (ROM name)
  out: V0 = pid, VF = 0/1 (error)

SYS 0x02: exit
  in:  V1 = status
  out: none

SYS 0x03: wait
  in:  V1 = pid
  out: V0 = exit code or 0xFF if still running

SYS 0x04: yield
  in:  none
  out: none
```

### Validation tests

- Unit tests for the dispatcher:
  - `0nnn` routes to the syscall table.
  - ABI argument/return registers are preserved.
  - Invalid syscall IDs set `VF` and return an error code.

### Current status

- Implemented: syscall table + dispatcher for `0nnn` in 0x0100..0x01FF, with tests.
- Implemented: kernel owner with cooperative scheduler, root-dir ROM resolution,
  and base syscall registration (spawn/exit/wait/yield/write/read).
- Implemented: syscall ABI reference (`SYSCALLS.md`) and syscall-level tests.
- Implemented: code tour reference for Rust concepts (`CODE_TOUR.md`).
- Remaining: build the CLI ROM to exercise I/O and spawn.

### Milestone success

- Syscall dispatch is stable and tested without requiring a full CLI ROM.

---

## Phase 2: CLI ROM + I/O Layer (Text Console + Input) (2-4 weeks)

Goal: a real CLI ROM that can spawn other programs, list available ROMs, and
handle basic text I/O.

### Options

- **Text console service** (host prints strings)
- **Shared console buffer** in memory (host renders a grid)
- **Hybrid** (host output + Chip-8 input)

### Additional requirements for a real CLI

- Host-backed filesystem syscalls (list/open/read/close) to enumerate ROMs.
- ROM-side helper routines for syscall frame construction and string parsing.

### Example syscall IDs

```
SYS 0x10: write
  in: V1 = ptr to buffer, V2 = len
  out: V0 = bytes written

SYS 0x11: read_key
  out: V0 = key or 0xFF if none
```

### Milestone success

- CLI can spawn another ROM and resume after it exits.
- CLI can print and read user commands reliably.
- CLI can list available ROMs from a host-backed filesystem.
- CLI includes ROM-side helper routines for syscall frames and string parsing.

### Current status

- In progress: c8asm-based CLI ROM under `roms/cli/` with a syscall helper
  library, display-backed console mode, and the initial command set
  (`help`, `ls`, `run`, `cat`, `exit`).

---

## Phase 3: Filesystem Prototype (4-6 weeks)

Goal: Chip-8 programs can list, open, read, and write files.

### Start small

- Host-backed filesystem rooted at the CLI root directory.
- Current constraints (enforced at startup):
  - filename length <= 64 bytes per path segment
  - max entries per directory = 256
  - max file size = 64 KB
  - max open files per process = 32

### Example syscall IDs

```
SYS 0x0120: fs_list
  in:  arg0 = path ptr, arg1 = path len, arg2 = out buf ptr, arg3 = max entries
  out: V0 = count

SYS 0x0121: fs_open
  in:  arg0 = path ptr, arg1 = path len, arg2 = flags (read-only for now)
  out: V0 = fd

SYS 0x0122: fs_read
  in:  arg0 = fd, arg1 = buf ptr, arg2 = len
  out: V0 = bytes read

SYS 0x0123: fs_close
  in:  arg0 = fd
  out: VF = 0/1
```

### Milestone success

- Implemented: fs_list/open/read/close (host-backed, root-constrained).
- Remaining: fs_write and any permission model.
- CLI can list files and run a ROM from the filesystem.

---

## Phase 4: Multi-Process Scheduling & Isolation (4-8 weeks)

Goal: multiple programs can run concurrently and communicate.

### Scheduling

- Round-robin across active processes.
- Cooperative yield first; later introduce a fixed instruction slice.

### Isolation

- Each process gets its own page(s).
- Add a shared page for IPC or console buffers.

### IPC Options

- Shared ring buffer.
- Syscalls for `ipc_send` / `ipc_recv`.

### Milestone success

- Two ROMs run in parallel and exchange messages.

---

## Phase 5: OS-Level Services (Optional, 6-12 weeks)

- Environment variables (key/value store).
- Working directory.
- Simple shell scripting.
- Job control (foreground/background tasks).

---

## Target Architecture (ASCII Diagram)

```
+------------------------------+
| Host Runtime (Rust)          |
|  - Scheduler                 |
|  - Syscall dispatcher        |
|  - Filesystem (virtual)      |
|  - Console / Input           |
+--------------+---------------+
               |
               | syscalls
               v
+------------------------------+      +------------------------------+
| Proc A (CLI)                 |      | Proc B (Program)             |
|  V regs, I, PC, SP           |      |  V regs, I, PC, SP            |
|  vpage0 -> phys 0x4???       |      |  vpage0 -> phys 0x5???        |
+------------------------------+      +------------------------------+
              ^                                  ^
              | shared page / ipc                |
              +----------------------------------+
```

---

## Known Risks and Pitfalls

- **Opcode conflicts** with classic ROMs: keep extensions explicit.
- **Timing sensitivity**: preemptive scheduling can alter ROM behavior.
- **Memory limits**: page counts constrain OS features; choose defaults that
  leave room for stacks, CLI buffers, and filesystem I/O.
- **Input determinism**: test stability can degrade if input polling is too complex.

---

## Recommended Immediate Next Step

1. Align `EXTENSION.md` and `ROADMAP.md` with the current paging/translation model.
2. Implement a minimal syscall dispatcher + syscall table with ABI tests.
3. Add a kernel/runtime struct to manage Proc lifecycle and syscall routing.
