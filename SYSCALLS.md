# Chip-8 Runtime Syscall ABI

This document is the canonical ABI reference for extended syscalls exposed by
the runtime. It complements `EXTENSION.md` with concrete frame examples.

---

## 1) Invocation

Syscalls are invoked via `0nnn` where `nnn` is the syscall ID. Only IDs in
`0x0100..0x01FF` are dispatched to the host.

```
0x00E0 = CLS
0x00EE = RET
0x0100..0x01FF = syscall dispatch
```

---

## 2) Register + Frame Conventions

```
V0  = return value (8-bit)
VF  = error flag (0 = OK, 1 = error)
I   = pointer to syscall frame (virtual address)
```

Frame layout (implemented):

```
I + 0: frame length (bytes, including this length byte)
I + 1: arg0_hi
I + 2: arg0_lo
I + 3: arg1_hi
I + 4: arg1_lo
I + 5: payload / additional args...
```

Arguments are 16-bit big-endian values. The kernel reads args by index:
`arg0`, `arg1`, `arg2`, ...

---

## 3) Error Conventions

### VF Register Contract

**All syscall handlers MUST set the VF register (V[0xF]) on every code path.**

This contract is critical for reliable trace error reporting and syscall observability.

**Success Path:**
```rust
proc.regs.V[0] = result_value;  // Return value (bytes written, PID, etc.)
proc.regs.V[0xF] = 0;           // Success flag (REQUIRED)
SyscallOutcome::Completed
```

**Error Path:**
```rust
proc.regs.V[0] = ERROR_CODE;    // Specific error code (see below)
proc.regs.V[0xF] = 1;           // Error flag (REQUIRED)
return SyscallOutcome::Completed;
```

**Blocked Path (sys_wait, sys_read):**
When a syscall blocks, VF is set when the syscall is unblocked:
```rust
// On unblock:
entry.proc.regs.V[0] = result_value;
entry.proc.regs.V[0xF] = 0;      // Set on successful unblock
```

### Error Codes

Error codes currently in use:

```
0x02 (ERR_INVALID)        = invalid argument or frame too small
0x03 (ERR_IO)             = I/O failure (host write/read)
0x04 (ERR_NOT_FOUND)      = resource not found (file, PID, etc.)
0x05 (ERR_NOT_DIR)        = not a directory
0x06 (ERR_IS_DIR)         = is a directory (when file expected)
0x07 (ERR_NAME_TOO_LONG)  = filename exceeds 64 bytes
0x08 (ERR_TOO_MANY_OPEN)  = file descriptor table full
0x09 (ERR_PATH)           = invalid path or path resolution failed
0x0A (ERR_STACK)          = stack overflow/underflow
```

**Contract Audit (Week 4):** All 18 syscall handlers have been audited and verified compliant with the VF contract. See Week 4 completion notes in MEMORY.md.

---

## 4) Base Syscall IDs

```
0x0101 = spawn
0x0102 = exit
0x0103 = wait
0x0104 = yield
0x0110 = write
0x0111 = read
0x0112 = input_mode
0x0113 = console_mode
0x0120 = fs_list
0x0121 = fs_open
0x0122 = fs_read
0x0123 = fs_close
0x0130 = dbg_list
0x0131 = dbg_regs
0x0132 = dbg_mem_read
0x0133 = dbg_mem_write
0x0134 = dbg_trace_read
```

---

## 5) Syscall Definitions + Frames

### 0x0101 spawn

Args:
```
arg0 = ptr to ROM name string
arg1 = string length
arg2 = page count (defaults to 1 if omitted)
```

Returns:
```
V0 = pid (low 8 bits)
VF = 0 on success, 1 on error
```

TODO:
- Extend PID return to 16-bit via frame or register pair.

Frame example:
```
I = 0x300
ROM name at 0x340 = "child.ch8"

frame bytes:
0x300: 0x07        ; length = 1 + (3 * 2)
0x301: 0x03 0x40   ; arg0 = 0x0340
0x303: 0x00 0x09   ; arg1 = 9
0x305: 0x00 0x01   ; arg2 = 1
```

Notes:
- ROMs are resolved relative to the kernel root directory.

### 0x0102 exit

Args:
```
arg0 = exit code
```

Returns:
```
VF = 0
```

Frame example:
```
I = 0x320
frame bytes:
0x320: 0x03        ; length = 1 + (1 * 2)
0x321: 0x00 0x2A   ; arg0 = 0x002A
```

### 0x0103 wait

Args:
```
arg0 = pid to wait on
```

Returns:
```
V0 = exit code
VF = 0 on success, 1 on error
```

Notes:
- The caller blocks until the target pid exits.

### 0x0104 yield

Args: none

Returns:
```
VF = 0
```

Notes:
- The caller yields to the scheduler.

### 0x0110 write

Args:
```
arg0 = buffer pointer
arg1 = length
```

Returns:
```
V0 = bytes written (low 8 bits)
VF = 0 on success, 1 on error
```

Frame example:
```
I = 0x300
buffer at 0x320 = "hello"

frame bytes:
0x300: 0x05        ; length = 1 + (2 * 2)
0x301: 0x03 0x20   ; arg0 = 0x0320
0x303: 0x00 0x05   ; arg1 = 5
```

### 0x0111 read

Args:
```
arg0 = buffer pointer
arg1 = length
```

Returns:
```
V0 = bytes read (low 8 bits)
VF = 0 on success, 1 on error
```

Notes:
- The caller blocks until input is available.
- Input can be **line-oriented** or **byte-exact** depending on `input_mode`.
- When `console_mode` is set to display, input/output is routed through the
  Chip-8 window instead of the host stdin/stdout.

### 0x0112 input_mode

Args:
```
arg0 = mode (0 = line, 1 = byte)
```

Returns:
```
VF = 0 on success, 1 on error
```

Notes:
- Line mode blocks until a newline is available and delivers up to the newline.
- Byte mode delivers any available bytes immediately.

### 0x0113 console_mode

Args:
```
arg0 = mode (0 = host stdin/stdout, 1 = display console)
```

Returns:
```
VF = 0 on success, 1 on error
```

Notes:
- Display mode renders text into the Chip-8 window using an 80x40 character grid
  (640x320 logical pixels, 8x8 font).
- Sprite drawing is ignored while in console mode.
- Input comes from the window keyboard and is echoed to the console.

---

## 6) Filesystem Syscalls (Host-backed)

All filesystem paths are resolved **relative to the kernel root directory**.
Absolute paths and `..` are rejected. At startup, the kernel validates the root
directory layout against the limits below and fails fast with a descriptive
error if any violation is found.

Limits (current):
```
MAX_FILENAME_LEN = 64 bytes (per path segment)
MAX_DIR_ENTRIES  = 256 entries per directory
MAX_FILE_SIZE    = 64 KB
MAX_OPEN_FILES   = 32 per process
```

Directory entry record layout (`fs_list` output):
```
name_len  : u8
name      : [u8; 64]   (padded with 0s)
kind      : u8         (0 = file, 1 = dir)
size_be   : u32        (big-endian, bytes; 0 for dirs)
```

Record size: `1 + 64 + 1 + 4 = 70` bytes.

### 0x0120 fs_list

Args:
```
arg0 = ptr to path string (relative, may be empty for root)
arg1 = path length
arg2 = out buffer pointer
arg3 = max entries
```

Returns:
```
V0 = entries written (low 8 bits)
VF = 0 on success, 1 on error
```

### 0x0121 fs_open

Args:
```
arg0 = ptr to path string (relative)
arg1 = path length
arg2 = flags (currently ignored; read-only)
```

Returns:
```
V0 = fd (8-bit)
VF = 0 on success, 1 on error
```

### 0x0122 fs_read

Args:
```
arg0 = fd
arg1 = buffer pointer
arg2 = length (bytes; max 255 per call)
```

Returns:
```
V0 = bytes read (low 8 bits; 0 at EOF)
VF = 0 on success, 1 on error
```

### 0x0123 fs_close

Args:
```
arg0 = fd
```

Returns:
```
VF = 0 on success, 1 on error
```

---

## 0x0130..0x013F — Debugger Interface (Initial ABI)

These syscalls provide an in-band debug interface so a Chip-8 ROM can inspect
other running processes. All operations are performed by the kernel; the debug
ROM provides a target pid and buffers in its own memory.

### 0x0130 dbg_list

Args:
```
arg0 = out buffer pointer
arg1 = max entries
```

Record layout (8 bytes per entry):
```
pid_be   : u32
state    : u8   (0 = running, 1 = blocked, 2 = exited)
exit     : u8   (0 if none)
reserved : u16  (0)
```

Returns:
```
V0 = entries written (low 8 bits)
VF = 0 on success, 1 on error
```

### 0x0131 dbg_regs

Args:
```
arg0 = target pid (low 16 bits)
arg1 = out buffer pointer
```

Record layout (24 bytes):
```
PC_be : u16
SP_be : u16
I_be  : u16
V[16] : u8 x16
DT    : u8
ST    : u8
```

Returns:
```
V0 = bytes written (24)
VF = 0 on success, 1 on error
```

### 0x0132 dbg_mem_read

Args:
```
arg0 = target pid (low 16 bits)
arg1 = target address (virtual)
arg2 = length
arg3 = out buffer pointer
```

Returns:
```
V0 = bytes read (low 8 bits)
VF = 0 on success, 1 on error
```

### 0x0133 dbg_mem_write

Args:
```
arg0 = target pid (low 16 bits)
arg1 = target address (virtual)
arg2 = length
arg3 = input buffer pointer
```

Returns:
```
V0 = bytes written (low 8 bits)
VF = 0 on success, 1 on error
```

### 0x0134 dbg_trace_read

Args:
```
arg0 = out buffer pointer
arg1 = max records to read
```

Returns:
```
V0 = records written (0 if none)
VF = 0 on success, 1 on error
```

Notes:
- Non-blocking: returns 0 if no records are available.
- Records are consumed from the kernel trace ring buffer.
- Each record is **8 bytes**.
- The kernel does not emit trace records for `dbg_trace_read` itself to avoid
  trace feedback loops.

Record layout (8 bytes):
```
byte0 = kind
byte1 = op
byte2..3 = pid (u16, big-endian, low 16 bits)
byte4..5 = arg0 (u16)
byte6..7 = arg1 (u16)
```

Kinds:
```
0x01 = scheduler event
0x02 = syscall event
```

Scheduler op codes:
```
0x01 = spawned
0x02 = unblocked
0x03 = yielded
0x04 = preempted
0x05 = blocked
0x06 = exited
```

Syscall op codes:
```
0x01 = completed
0x02 = yielded
0x03 = blocked
0x04 = error
```

---

## 7) Headless Mode (Testing)

If `CHIP8_HEADLESS` is set in the environment, new displays are created without
opening a window. This is intended for tests and CI.
