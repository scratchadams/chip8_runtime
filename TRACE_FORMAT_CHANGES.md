# Trace Record Format Changes (Week 3 Task 3.4)

## Overview

Trace records now include error codes for syscall errors, stored in the arg1 field (bytes 6-7). This enables the debugger ROM to identify which specific error occurred for each failed syscall.

## Previous Format

```
Byte 0:    kind (0x02 = TRACE_KIND_SYSCALL)
Byte 1:    outcome (0x01=completed, 0x02=yielded, 0x03=blocked, 0x04=error)
Bytes 2-3: pid (u16, big-endian)
Bytes 4-5: syscall_id (u16, big-endian)
Bytes 6-7: unused (always 0x0000)
```

## New Format (Week 3+)

```
Byte 0:    kind (0x02 = TRACE_KIND_SYSCALL)
Byte 1:    outcome (0x01=completed, 0x02=yielded, 0x03=blocked, 0x04=error)
Bytes 2-3: pid (u16, big-endian)
Bytes 4-5: syscall_id (u16, big-endian)
Bytes 6-7: error_code (u16, big-endian) - 0x0000 for success, error code otherwise
```

### Error Code Mapping

When outcome = 0x04 (error) OR when VF=1 after syscall completion:

| Code | Name | Description |
|------|------|-------------|
| 0x0002 | ERR_INVALID | Invalid argument or frame too small |
| 0x0003 | ERR_IO | I/O operation failed |
| 0x0004 | ERR_NOT_FOUND | Resource not found |
| 0x0005 | ERR_NOT_DIR | Path is not a directory |
| 0x0006 | ERR_IS_DIR | Path is a directory when file expected |
| 0x0007 | ERR_NAME_TOO_LONG | Filename exceeds 64 bytes |
| 0x0008 | ERR_TOO_MANY_OPEN | File descriptor table full |
| 0x0009 | ERR_PATH | Path resolution failed |
| 0x000A | ERR_STACK | Stack overflow/underflow |

## Implementation Details

### Kernel Changes

1. **trace_syscall() signature updated:**
   ```rust
   fn trace_syscall(&mut self, pid: u32, id: u16, outcome: u8, error_code: u8)
   ```

2. **dispatch_syscall() captures error codes:**
   ```rust
   // After syscall executes, check VF register
   let error_code = if proc.regs.V[0xF] == 1 { proc.regs.V[0] } else { 0 };
   self.trace_syscall(pid, id, trace_outcome, error_code);
   ```

3. **Unknown syscall errors:**
   ```rust
   // Syscall ID not registered
   self.trace_syscall(pid, id, TRACE_SYSCALL_ERROR, ERR_INVALID);
   ```

### Trace Record Examples

**Successful syscall:**
```
02 01 00 01  01 10 00 00  = sys_write (0x0110) completed successfully
                       ^^
                       error_code = 0x00 (success)
```

**Failed syscall:**
```
02 01 00 01  01 10 00 02  = sys_write (0x0110) completed with ERR_INVALID
                       ^^
                       error_code = 0x02 (ERR_INVALID)
```

**Unknown syscall:**
```
02 04 00 01  FF FF 00 02  = unknown syscall (0xFFFF) error ERR_INVALID
 ^^ ^^
 kind=syscall, outcome=error
```

## Required Debugger ROM Changes

### Current Decoder (roms/debugger.ch8)

The current debugger ROM decodes trace records as follows (estimated):

```
// Read 8-byte trace record
I = trace_buffer
LD V0, [I]  // V0=kind, V1=op, V2=pid_hi, V3=pid_lo, V4=arg0_hi, V5=arg0_lo, V6=arg1_hi, V7=arg1_lo

// Decode syscall traces
SE V0, 0x02          // if kind == TRACE_KIND_SYSCALL
SE V1, 0x04          // if outcome == ERROR
// Display syscall_id from V4:V5
// V6:V7 were previously ignored
```

### Required Updates

**1. Display error codes for failed syscalls:**

```
// After decoding syscall trace
SE V1, 0x01          // if outcome == COMPLETED
JP check_vf          // Check if VF indicates error
...

check_vf:
LD V8, I             // Save I
I = proc_regs_addr
LD VA, [I]           // Load VF from process registers
SE VA, 0x01          // if VF == 1 (error)
JP display_error     // Show error code from V6:V7

display_error:
// V6:V7 contain error_code
// Map to error name and display
I = error_table
ADD I, V6            // Offset by error code
LD VB, [I]           // Load error name string
CALL display_string
```

**2. Error code lookup table:**

```
error_table:
  DB 0x00  // 0x00: (success - skip)
  DB 0x00  // 0x01: (unused)
  DB "ERR_INVALID", 0
  DB "ERR_IO", 0
  DB "ERR_NOT_FOUND", 0
  DB "ERR_NOT_DIR", 0
  DB "ERR_IS_DIR", 0
  DB "ERR_NAME_TOO_LONG", 0
  DB "ERR_TOO_MANY_OPEN", 0
  DB "ERR_PATH", 0
  DB "ERR_STACK", 0
```

**3. Enhanced trace display:**

Instead of just showing:
```
[SYSCALL] pid=1 sys_write ERROR
```

Now show:
```
[SYSCALL] pid=1 sys_write ERROR (ERR_INVALID: buffer read failed)
```

### Backwards Compatibility

**Breaking changes:**
- Old debugger ROMs will ignore error codes (bytes 6-7 were previously unused)
- No functional breakage, but diagnostic information is lost
- Upgrading debugger ROM is recommended but not required

**Non-breaking changes:**
- Trace record size unchanged (8 bytes)
- kind, outcome, pid, syscall_id fields unchanged
- Only arg1 field (previously unused) now populated

## Testing

Verify error code capture with:

```bash
# Enable syscall error logging
export CHIP8_SYSCALL_ERRORS=1

# Run ROM that triggers syscall errors
./chip8_runtime bad_rom.ch8 2>errors.log

# Check trace buffer includes error codes
# (Requires debugger ROM or custom test)
```

## Documentation Updates

- SYSCALLS.md: No changes (error codes already documented)
- EXTENSION.md: No changes (trace format extension documented here)
- debugger.ch8 source: Update decode logic (see Required Changes above)

## Future Enhancements

1. **Richer error context**: Store additional error data in extended trace records (16+ bytes)
2. **Error rate metrics**: Aggregate error codes in kernel for monitoring
3. **Error-specific recovery**: Syscalls could specify retry/fallback strategies

---

**Generated as part of Week 3: Polish & Production Readiness (Phase 1.5 → Phase 2)**
