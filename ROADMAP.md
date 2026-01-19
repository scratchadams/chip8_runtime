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

- `EXTENSION.md` describing opcode encodings, syscall IDs, and the ABI.

---

## Phase 1: Minimal Syscalls + CLI (2-4 weeks)

Goal: a Chip-8 CLI program that can spawn and manage other ROMs.

### Required runtime changes

- Syscall dispatcher with a stable ABI.
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

### CLI ROM

- A small Chip-8 program that:
  - lists available programs (later)
  - spawns a selected ROM
  - waits for completion
  - prints status

### Milestone success

- CLI can spawn another ROM and resume after it exits.

---

## Phase 2: I/O Layer (Text Console + Input) (2-4 weeks)

Goal: make the CLI practical without complex sprite rendering.

### Options

- **Text console service** (host prints strings)
- **Shared console buffer** in memory (host renders a grid)
- **Hybrid** (host output + Chip-8 input)

### Example syscall IDs

```
SYS 0x10: write
  in: V1 = ptr to buffer, V2 = len
  out: V0 = bytes written

SYS 0x11: read_key
  out: V0 = key or 0xFF if none
```

### Milestone success

- CLI can print and read user commands reliably.

---

## Phase 3: Filesystem Prototype (4-6 weeks)

Goal: Chip-8 programs can list, open, read, and write files.

### Start small

- Host-backed virtual filesystem (or RAM disk).
- Small constraints:
  - filename length 8-16 bytes
  - max files 32
  - file size limit 512-2K bytes

### Example syscall IDs

```
SYS 0x20: fs_list
  in:  V1 = buffer ptr, V2 = max entries
  out: V0 = count

SYS 0x21: fs_open
  in:  V1 = ptr to name, V2 = flags
  out: V0 = fd

SYS 0x22: fs_read
  in:  V1 = fd, V2 = buf ptr, V3 = len
  out: V0 = bytes read

SYS 0x23: fs_write
  in:  V1 = fd, V2 = buf ptr, V3 = len
  out: V0 = bytes written

SYS 0x24: fs_close
  in:  V1 = fd
  out: V0 = status
```

### Milestone success

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

1. Add `EXTENSION.md` defining the syscall ABI and opcode encoding.
2. Implement a minimal syscall dispatcher with `spawn`, `exit`, `yield`, `write`.
3. Create a small CLI ROM that exercises those calls.
