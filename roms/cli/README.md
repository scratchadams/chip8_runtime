# Chip-8 CLI ROM (c8asm)

This directory contains a small CLI ROM written in the project’s **structured
block** assembly (`.c8s`). It is intended as a living example of how to drive
the runtime syscalls from inside a Chip-8 program, plus a reusable syscall
helper library (`lib/sys.c8s`).

The CLI currently supports five commands:

```
help
ls
run <rom>
cat <file>
exit
```

The ROM uses **line-oriented** input mode and parses a single command line into
`tok1` (the command) and an optional `tok2` argument. It also opts into the
display-backed console so input/output happens inside the Chip-8 window.

---

## 1) Build + Run

This project uses the in-repo assembler (`c8asm`). The build script concatenates
the CLI, library, and data files into a single `.c8s` source and then assembles
it into a `.ch8` ROM.

```
roms/cli/build.sh
```

This produces:

```
roms/cli/build/cli_combined.c8s
```

The assembled ROM is written to:

```
roms/cli/build/cli.ch8
```

Run it with the runtime:

```
cargo run -- --root /path/to/rom/root roms/cli/build/cli.ch8
```

(Replace paths with your local layout.)

---

## 2) File Layout

```
roms/cli/
  cli.c8s         # main program + command dispatch
  lib/sys.c8s     # syscall frame builders + wrappers
  lib/data.c8s    # fixed buffers + constant strings
  build.sh        # concatenates + assembles into build/cli.ch8
```

The files are concatenated in this order:

1) `cli.c8s` (main program, starts at 0x200)
2) `lib/sys.c8s` (subroutines)
3) `lib/data.c8s` (buffers + strings placed at fixed addresses)

---

## 3) c8asm Syntax (Quick Reference)

The CLI ROM is written in **structured blocks**. Each section has a fixed start
address, and labels group instructions or data for readability.

```
section code @ 0x200 {
  label main {
    v1 := 0x00
    call sys_input_mode
    jump main
  }
}

section data @ 0x800 {
  LINE_BUF: zero 0x50
}
```

Minimal instruction forms used by the CLI:

- `vX := 0xNN` / `vX := vY`
- `vX += 0xNN` / `vX += vY`
- `vX -= 0xNN` (wraps modulo 256)
- `i := 0xNNN` / `i := LABEL`
- `i += vX`
- `call label`, `jump label`, `return`
- `if vX == 0xNN then jump label`
- `if vX != vY then jump label`
- `save vX`, `load vX`

Data directives:

- `byte ...`
- `word ...`
- `zero N`
- `ascii \"...\"`
- `sys 0xNNN` (emit raw `0nnn` opcode)

Syntax is **case-sensitive**. Keywords are lowercase and registers are `v0..vF`
(uppercase hex digits).

---

## 4) Memory Map (Fixed Addresses)

Chip-8 has no general “load 16-bit pointer into I from registers” instruction.
To keep the ROM simple, the CLI uses **fixed buffer addresses** with low bytes
chosen to avoid carry when adding offsets.

```
0x800 LINE_BUF   (80 bytes)  - input line buffer
0x850 FRAME      (16 bytes)  - syscall frame scratch
0x900 DIR_BUF    (0x118)     - fs_list output (4 entries max)
0xA20 FILE_BUF   (0x40)      - fs_read chunk buffer

0xB00 PROMPT     "> "
0xB10 WELCOME    "chip8 cli ready\n"
0xB40 HELP_TEXT  (86 bytes, multi-line help)
0xBA0 ERR_UNKNOWN "unknown command\n"
0xBB0 ERR_USAGE_RUN "usage: run <rom>\n"
0xBC1 ERR_USAGE_CAT "usage: cat <file>\n"
0xBD4 ERR_GENERIC  "error\n"
0xBE0 SLASH      "/"
0xBE1 NEWLINE    "\n"
```

**Why the low bytes matter:**
- `LINE_BUF` is at `0x800` so `LINE_BUF + tok_offset` does not change the high
  byte. This makes it easy to build pointers by just setting `arg0_hi = 0x08`
  and `arg0_lo = tok_offset`.
- `DIR_BUF` is at `0x900` for the same reason; each directory record offset fits
  in a single byte and does not carry into the high byte.

---

## 5) Syscall Library (`lib/sys.c8s`)

The library contains two layers:

### A) Frame builders

Each syscall reads a **frame** from memory via `I`. The frame format is:

```
I + 0: length (bytes, including this length byte)
I + 1: arg0_hi
I + 2: arg0_lo
I + 3: arg1_hi
I + 4: arg1_lo
I + 5: arg2_hi
I + 6: arg2_lo
I + 7: arg3_hi
I + 8: arg3_lo
```

The library provides helpers to write these frames into `FRAME`:

- `frame1` → length 3 (arg0)
- `frame2` → length 5 (arg0, arg1)
- `frame3` → length 7 (arg0..arg2)
- `frame4` → length 9 (arg0..arg3)

### B) Syscall wrappers

The wrappers set `I` to `FRAME`, emit the `0nnn` trap via the `sys` directive,
and return.
All wrappers use the same argument convention:

```
arg0 = v1 (hi), v2 (lo)
arg1 = v3 (hi), v4 (lo)
arg2 = v5 (hi), v6 (lo)
arg3 = v7 (hi), v8 (lo)
```

Wrappers provided:

- `sys_spawn`
- `sys_exit`
- `sys_wait`
- `sys_yield`
- `sys_write`
- `sys_read`
- `sys_input_mode`
- `sys_console_mode`
- `sys_fs_list`
- `sys_fs_open`
- `sys_fs_read`
- `sys_fs_close`

**Register safety:**
- `v0` is scratch inside the frame builders.
- `v1..v8` are inputs.
- `v9..vE` are untouched by the library.
- Syscalls write back to `V0` and `VF` per ABI.

---

## 6) CLI Control Flow (High Level)

```
main:
  set console_mode(display)
  set input_mode(line)
  print welcome
  loop:
    print prompt
    read line into LINE_BUF
    tokenize into (tok1, tok2)
    dispatch:
      help  -> print help
      ls    -> fs_list + print names
      run   -> spawn + wait
      cat   -> fs_open + fs_read + write + fs_close
      exit  -> sys_exit
```

### Tokenization strategy

We parse the input line by scanning bytes in `LINE_BUF`:

1) Skip leading spaces.
2) Mark `tok1` start and length until the next space.
3) Skip spaces.
4) Mark `tok2` start and length until the next space.

All offsets are measured relative to `LINE_BUF` (0x800), so building pointers
for syscalls is as simple as setting `arg0_hi = 0x08` and `arg0_lo = tok_off`.

---

## 7) CLI Command Notes

### `help`
Prints `HELP_TEXT`.

### `ls`
Calls:
```
fs_list("", 0, DIR_BUF, 4)
```
The CLI prints each name from the returned records. If the entry is a directory
(`kind == 1`), it appends `/`.

### `run <rom>`
Calls:
```
spawn(tok2_ptr, tok2_len, pages=1)
wait(pid)
```
The CLI blocks until the child exits.

### `cat <file>`
Calls:
```
fd = fs_open(tok2_ptr, tok2_len, flags=0)
loop:
  n = fs_read(fd, FILE_BUF, 0x40)
  if n == 0: break
  write(FILE_BUF, n)
fs_close(fd)
```

### `exit`
Calls `sys_exit(0)` and then halts in place until the scheduler removes the
process.

---

## 8) Why the ROM is Structured This Way

The main goals were clarity and maintainability:

- **Fixed buffer addresses** avoid complicated pointer math in raw Chip-8.
- **Frame builders** make the syscall ABI explicit in one place.
- **Small, named helpers** keep the main loop easy to read.
- **Documented constraints** make the ROM a reference for future ROMs.

If you extend the CLI, update the memory map and string table first. That keeps
command logic from subtly drifting away from the ABI.
