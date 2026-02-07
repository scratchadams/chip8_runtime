# Syscall Error Logging

## Overview

As of Week 3 Task 3.2, syscall errors are logged in JSON format to stderr for debugging and monitoring. Logging is always compiled in but runtime-gated by environment variable.

## Enabling Logging

Set the `CHIP8_SYSCALL_ERRORS` environment variable to enable logging:

```bash
export CHIP8_SYSCALL_ERRORS=1
./chip8_runtime myrom.ch8 2>errors.log
```

## Log Format

Logs are emitted as single-line JSON objects to stderr:

```json
{"level":"error","timestamp":1739048096789,"event":"syscall_error","pid":1,"syscall_id":"0x0110","syscall_name":"sys_write","error_code":"0x02","error_name":"ERR_INVALID","message":"buffer read failed (out of bounds)"}
```

### Fields

- `level`: Always "error" for syscall errors
- `timestamp`: Unix timestamp in milliseconds
- `event`: Always "syscall_error"
- `pid`: Process ID that triggered the error
- `syscall_id`: Syscall ID in hexadecimal (e.g., "0x0110" for sys_write)
- `syscall_name`: Human-readable syscall name (e.g., "sys_write")
- `error_code`: Error code in hexadecimal (e.g., "0x02" for ERR_INVALID)
- `error_name`: Human-readable error name (e.g., "ERR_INVALID")
- `message`: Specific error message describing what went wrong

## Error Codes

| Code | Name | Description |
|------|------|-------------|
| 0x02 | ERR_INVALID | Invalid argument or frame too small |
| 0x03 | ERR_IO | I/O operation failed |
| 0x04 | ERR_NOT_FOUND | Resource not found (file, PID, etc.) |
| 0x05 | ERR_NOT_DIR | Path is not a directory |
| 0x06 | ERR_IS_DIR | Path is a directory when file expected |
| 0x07 | ERR_NAME_TOO_LONG | Filename exceeds 64 bytes |
| 0x08 | ERR_TOO_MANY_OPEN | File descriptor table full |
| 0x09 | ERR_PATH | Path resolution failed |
| 0x0A | ERR_STACK | Stack overflow/underflow |

## Covered Syscalls

Currently instrumented syscalls (Week 3 Task 3.2 Phase 1):

- **sys_spawn (0x0101)**: Process spawning errors
  - Frame too small for name_ptr/name_len
  - Name buffer read failed
  - ROM path resolution failed
  - Display initialization failed
  - Process spawn failed

- **sys_wait (0x0103)**: Process wait errors
  - Frame too small for target PID
  - Target PID not found

- **sys_write (0x0110)**: Output errors
  - Frame too small for buf/len
  - Buffer read failed (out of bounds)
  - Host output write failed

- **sys_read (0x0111)**: Input errors
  - Frame too small for buf/len
  - Buffer write failed (out of bounds) - 4 code paths

- **sys_input_mode (0x0112)**: Input mode errors
  - Frame too small for mode
  - Invalid mode value (must be 0 or 1)

- **sys_console_mode (0x0113)**: Console mode errors
  - Frame too small for mode
  - Invalid mode value (must be 0 or 1)

## Future Work

Additional syscalls to instrument (Week 3 Task 3.2 Phase 2):
- sys_fs_list, sys_fs_open, sys_fs_read, sys_fs_close (filesystem errors)
- sys_dbg_* (debugger syscall errors)

## Implementation Details

### Architecture

- **Always compiled**: Logging code is not feature-gated, ensuring zero divergence between debug and release builds
- **Runtime gated**: `CHIP8_SYSCALL_ERRORS` env var checked once per log call (minimal overhead when disabled)
- **JSON format**: Structured logs enable parsing by log aggregators (Splunk, ELK, Datadog)
- **Stderr output**: Separates error logs from application stdout

### Code Location

- **Helper functions**: `src/kernel.rs` lines 73-133
  - `syscall_name()`: Maps syscall ID to human name
  - `error_name()`: Maps error code to human name
  - `log_syscall_error()`: Formats and emits JSON log

- **Instrumentation**: Each syscall handler calls `log_syscall_error()` before setting error flags

### Performance Impact

- **Disabled (default)**: One env var check per error (~10ns overhead, negligible)
- **Enabled**: JSON formatting + stderr write (~10μs per error)
- **No impact on success path**: Logging only occurs on error returns

## Example Usage

### Debugging Buffer Overflows

```bash
$ CHIP8_SYSCALL_ERRORS=1 ./chip8_runtime bad_rom.ch8 2>&1 >/dev/null | jq .
{
  "level": "error",
  "timestamp": 1739048096789,
  "event": "syscall_error",
  "pid": 1,
  "syscall_id": "0x0110",
  "syscall_name": "sys_write",
  "error_code": "0x02",
  "error_name": "ERR_INVALID",
  "message": "buffer read failed (out of bounds)"
}
```

### Monitoring Production

```bash
# Aggregate errors by syscall
CHIP8_SYSCALL_ERRORS=1 ./chip8_runtime *.ch8 2>&1 >/dev/null \
  | jq -r '.syscall_name' | sort | uniq -c | sort -rn

# Find processes with most errors
CHIP8_SYSCALL_ERRORS=1 ./chip8_runtime *.ch8 2>&1 >/dev/null \
  | jq -r '.pid' | sort | uniq -c | sort -rn
```

## Testing

See `tests/syscall_error_logging.rs` for integration tests:
- Verifies logging when env var is set
- Verifies no-op when env var is not set

Run tests:
```bash
cargo test syscall_error_logging
```

## Related Documentation

- [SYSCALLS.md](SYSCALLS.md): Syscall ABI and error code definitions
- [EXTENSION.md](EXTENSION.md): Columbia CHIP-8 specification extensions
