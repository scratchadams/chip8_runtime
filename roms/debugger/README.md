# Chip-8 Debugger ROM (Trace Viewer)

This ROM reads the kernel trace ring buffer via `dbg_trace_read` and prints
**decoded** scheduler/syscall events to its own display-backed console window.
It is a minimal, read-only debugger intended to run alongside other ROMs.

Each trace record is 8 bytes (see `SYSCALLS.md` for the format). The ROM
prints decoded lines such as:

```
sched pid=0001 op=03
sys pid=0001 id=0104 st=02
```

On startup it also performs a one-time inspection pass:

- `dbg_list` to pick a target pid
- `dbg_regs` to dump PC/SP/I
- `dbg_mem_read` to dump 0x0200..0x020F

---

## Build

```
roms/debugger/build.sh
```

This produces:

```
roms/debugger/build/debugger.ch8
```

---

## Run (with another ROM)

Example: run the CLI plus the debugger ROM so the debugger gets its own window:

```
cargo run --bin chip8_runtime -- --root /path/to/rom/root \
  roms/cli/build/cli.ch8 \
  roms/debugger/build/debugger.ch8
```

---

## Filters (startup prompt)

On boot the ROM prompts once for filter settings:

```
filter a/s/y HH|-
```

- `a` = all events, `s` = scheduler only, `y` = syscall only
- `HH` = pid in hex (two digits), or `-` for no pid filter

Examples:
- `a -`   → show all events
- `s 01`  → only scheduler events for pid 0x01
- `y 02`  → only syscall events for pid 0x02

These filters are **read once at startup**; restart the ROM to change them.

---

## Notes

- The debugger ROM is read-only (v1).
- Records are consumed from the kernel ring buffer as they are read.
- The ROM reads batches of 8 records per poll.
