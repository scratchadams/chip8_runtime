# Chip-8 OS Extensions Specification (Draft)

This document defines a **detailed, explicit extension layer** for your
Chip-8 runtime so that programs can behave like a small OS. It focuses on:

- A syscall ABI using the legacy `0nnn` opcode space
- Stack manipulation syscalls (PUSH/POP/SPGET/SPSET)
- Multi-page virtual address space while **preserving shared physical memory**
- Memory and error conventions that are simple for Chip-8 ROMs to implement

The goal is to keep classic Chip-8 compatibility, while enabling richer,
OS-like behaviors.

---

## 1) Guiding Goals

1. **Compatibility first**
   - Classic Chip-8 ROMs should still run without change.
   - Extensions are explicit: no ambiguity with standard opcodes.

2. **Shared physical memory model**
   - All process memory (including stacks) remains in `SharedMemory.phys_mem`.
   - Processes see a larger *virtual* address space that is mapped to physical.

3. **Small, predictable ABI**
   - Registers for small args, memory for structured data.
   - Error handling consistent across syscalls.

---

## 2) Syscall Invocation: 0nnn (SYS table)

We repurpose `0nnn` as a syscall gateway. This opcode is historically "SYS addr"
and ignored by most modern interpreters, which makes it a low-risk extension
space.

### 2.1 Dispatch Rule

```
0nnn:
  nnn is treated as a syscall ID (0x000..0xFFF)
  - 00E0 and 00EE remain CLS/RET (hard-coded)
  - Other 0nnn values call the syscall dispatcher
```

### 2.2 Rationale

- `0nnn` is a known "escape hatch" in the classic spec.
- It avoids collisions with standard opcodes.
- It provides a clean, uniform entry to OS functionality.

---

## 3) Syscall ABI (Register + Memory Frame)

### 3.1 Register Conventions

```
V0  = return value (8-bit)
V1..VE = arguments (8-bit)
VF  = error flag (0 = OK, 1 = error)
I   = pointer to syscall argument frame (page-relative)
```

### 3.2 Memory Frame

Use `I` as a pointer to a **syscall frame** for larger values or buffers.
This keeps the register ABI small but still allows 16-bit and variable-sized
arguments.

#### Example frame layout (recommended)

```
I + 0: frame length (bytes)
I + 1: arg0_hi
I + 2: arg0_lo
I + 3: arg1_hi
I + 4: arg1_lo
I + 5: payload...
```

The exact frame layout is flexible; what matters is that it is documented and
consistent.

---

## 4) Stack Syscalls (Detailed)

Because Chip-8 lacks direct PUSH/POP opcodes, we define explicit syscalls in
the `0nnn` space. This gives programs richer stack control while keeping the
instruction set simple.

### 4.1 Syscall IDs (Proposal)

```
0x0F01  SYS_PUSH8    : push 8-bit value
0x0F02  SYS_POP8     : pop 8-bit value
0x0F03  SYS_PUSH16   : push 16-bit value
0x0F04  SYS_POP16    : pop 16-bit value
0x0F05  SYS_SP_GET   : read SP
0x0F06  SYS_SP_SET   : write SP
```

### 4.2 Argument and Return Conventions

**SYS_PUSH8 (0x0F01)**
- Input:
  - V1 = value to push
- Output:
  - V0 = 0 (unused)
  - VF = 0 if OK, 1 if stack overflow

**SYS_POP8 (0x0F02)**
- Input:
  - none
- Output:
  - V0 = popped value
  - VF = 0 if OK, 1 if stack underflow

**SYS_PUSH16 (0x0F03)**
- Input:
  - V1 = high byte
  - V2 = low byte
- Output:
  - VF = 0 if OK, 1 if stack overflow

**SYS_POP16 (0x0F04)**
- Input:
  - none
- Output:
  - V0 = high byte
  - V1 = low byte
  - VF = 0 if OK, 1 if stack underflow

**SYS_SP_GET (0x0F05)**
- Input: none
- Output:
  - V0 = SP high byte
  - V1 = SP low byte
  - VF = 0

**SYS_SP_SET (0x0F06)**
- Input:
  - V1 = SP high byte
  - V2 = SP low byte
- Output:
  - VF = 0 if OK, 1 if out of range

### 4.3 Stack Direction

Recommended convention for extended stacks:

```
Stack grows downward
SP points to the next free byte below the top
```

This avoids collisions with code/data and allows a clean “top-of-virtual-space”
stack.

---

## 5) Multi-Page Virtual Address Space

To allow larger stacks (and larger programs), expand each process’s virtual
address space from 4KB to N * 4KB pages. The runtime now implements a
single-level page table for this mapping.

### 5.1 Virtual Address Layout (Example: 8 pages = 32KB)

```
Virtual space: 0x0000 .. 0x7FFF

0x0000 .. 0x00FF  : fonts / OS data
0x0200 .. 0x5FFF  : program code + data
0x6000 .. 0x7FFF  : stack (grows downward)
```

### 5.2 Translation Strategy (Preserves Shared Physical Memory)

We keep `SharedMemory.phys_mem` as the backing store. The process’s virtual
address is translated to physical by:

1. Virtual page = `addr / PAGE_SIZE`
2. Page table yields physical page base
3. Physical = `phys_page_base + (addr % PAGE_SIZE)`


### 5.3 Current Status / Required Changes

1. **SharedMemory allocator**
   - Implemented: bitmap is `Vec<bool>` and `mmap(pages)` allocates N pages.
   - Still needed: `munmap()` or free list for process teardown.

2. **Proc structure**
   - Implemented: `page_table`, `page_count`, and `vm_size`.
   - Still needed: explicit stack bounds (`stack_top = vm_size - 1`).

3. **Memory access helpers**
   - Implemented: `translate()` plus `read_u8`/`write_u8` helpers.
   - Still needed: centralize bounds checks and error reporting.

4. **Stack bounds enforcement**
   - Still needed: enforce `stack_limit` and `stack_top` on call/return and
     on any future stack syscalls. Return VF=1 on overflow/underflow.

---

## 6) Example Syscall Use (Chip-8 Pseudocode)

### Push / Pop 16-bit value

```
; push 0x1234
V1 = 0x12
V2 = 0x34
0x0F03        ; SYS_PUSH16
; check VF for overflow

; pop 16-bit value -> V0:V1
0x0F04        ; SYS_POP16
; V0=hi, V1=lo
```

### Get/Set Stack Pointer

```
0x0F05        ; SYS_SP_GET
; V0 = SP hi, V1 = SP lo

V1 = 0x7F
V2 = 0xF0
0x0F06        ; SYS_SP_SET (SP = 0x7FF0)
```

---

## 7) Error Handling

All syscalls must follow a uniform error convention:

```
VF = 0  => success
VF = 1  => failure
V0 may contain a numeric error code when VF=1
```

Suggested error codes:

```
0x01 = invalid syscall ID
0x02 = invalid argument
0x03 = stack overflow
0x04 = stack underflow
0x05 = invalid address
0x06 = permission denied
```

---

## 8) Compatibility Notes

- Classic Chip-8 ROMs that use 0nnn are rare; most interpreters ignore it.
- Do not reuse `00E0` or `00EE` for syscalls.
- If you later want SCHIP/XO-CHIP support, reserve their known opcodes
  (00FB/00FC/00FD/00FE/00FF, and high-res instructions).

---

## 9) Summary

This extension layer gives you:

- A **clean syscall interface** using `0nnn`
- A **stack manipulation API** without altering the base instruction set
- A **scalable address space** while preserving shared physical memory

The approach minimizes compatibility risk and provides a clear path toward
OS-like Chip-8 programs (CLI, filesystem, process control) without rewriting
the emulator core.

If you want, the next step is to formalize the syscall IDs and implement a
minimal dispatcher in `chip8_engine.rs` that recognizes `0nnn` and routes to
host-side handlers.
