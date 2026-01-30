# Chip-8 Debugger ROM (Trace Viewer)

This ROM reads the kernel trace ring buffer via `dbg_trace_read` and prints
hex dumps of each trace record to its own display-backed console window. It is
intended as a minimal, read-only debugger that can run alongside other ROMs.

Each trace record is 8 bytes (see `SYSCALLS.md` for the format). The ROM
prints a header that explains the layout and then emits each record as:

```
AA AA AA AA AA AA AA AA\n
```

(one line per record).

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

## Notes

- The debugger ROM is read-only (v1).
- Records are consumed from the kernel ring buffer as they are read.
- The ROM uses a fixed read batch size of 8 records per poll.
