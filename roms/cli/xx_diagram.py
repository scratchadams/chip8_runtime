#!/usr/bin/env python3
"""
Generate or verify the CLI ROM xx diagram.

This script emits roms/cli/cli.xx from roms/cli/build/cli.ch8, adding
box-drawing annotations that explain structure, syscalls, and opcodes.
It can also verify that the xx file round-trips back into the same ROM
bytes using a minimal parser (and optionally the upstream xx.py parser).
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path

BASE_ADDR = 0x200
CODE_END = 0x596
PAD_END = 0x800

SYS_CALLS = {
    0x0101: "sys_spawn",
    0x0102: "sys_exit",
    0x0103: "sys_wait",
    0x0104: "sys_yield",
    0x0110: "sys_write",
    0x0111: "sys_read",
    0x0112: "sys_input_mode",
    0x0113: "sys_console_mode",
    0x0120: "sys_fs_list",
    0x0121: "sys_fs_open",
    0x0122: "sys_fs_read",
    0x0123: "sys_fs_close",
}

# Comment delimiters in xx; strings containing these must be emitted as hex.
COMMENT_CHARS = {"#", ";", "%", "|", "-", "/"}

BOX = {
    "tl": "\u250c",
    "tr": "\u2510",
    "bl": "\u2514",
    "br": "\u2518",
    "h": "\u2500",
    "v": "\u2502",
    "dtl": "\u2554",
    "dtr": "\u2557",
    "dbl": "\u255a",
    "dbr": "\u255d",
    "dh": "\u2550",
    "dv": "\u2551",
}

WIDTH = 76  # interior width (total line length is WIDTH + 2)


def materialize(text: str) -> bytes:
    return text.encode("utf-8").decode("unicode_escape").encode("latin1")


def decode(op: int) -> str:
    nnn = op & 0x0FFF
    n = op & 0x000F
    x = (op >> 8) & 0xF
    y = (op >> 4) & 0xF
    kk = op & 0xFF
    if op == 0x00E0:
        return "CLS"
    if op == 0x00EE:
        return "RET"
    if (op & 0xF000) == 0x0000:
        if nnn in SYS_CALLS:
            return f"SYS {nnn:#05x} ({SYS_CALLS[nnn]})"
        return f"SYS {nnn:#05x}"
    if (op & 0xF000) == 0x1000:
        return f"JP {nnn:#05x}"
    if (op & 0xF000) == 0x2000:
        return f"CALL {nnn:#05x}"
    if (op & 0xF000) == 0x3000:
        return f"SE V{x:X}, {kk:#04x}"
    if (op & 0xF000) == 0x4000:
        return f"SNE V{x:X}, {kk:#04x}"
    if (op & 0xF000) == 0x5000 and n == 0:
        return f"SE V{x:X}, V{y:X}"
    if (op & 0xF000) == 0x6000:
        return f"LD V{x:X}, {kk:#04x}"
    if (op & 0xF000) == 0x7000:
        return f"ADD V{x:X}, {kk:#04x}"
    if (op & 0xF000) == 0x8000:
        if n == 0x0:
            return f"LD V{x:X}, V{y:X}"
        if n == 0x1:
            return f"OR V{x:X}, V{y:X}"
        if n == 0x2:
            return f"AND V{x:X}, V{y:X}"
        if n == 0x3:
            return f"XOR V{x:X}, V{y:X}"
        if n == 0x4:
            return f"ADD V{x:X}, V{y:X}"
        if n == 0x5:
            return f"SUB V{x:X}, V{y:X}"
        if n == 0x6:
            return f"SHR V{x:X}"
        if n == 0x7:
            return f"SUBN V{x:X}, V{y:X}"
        if n == 0xE:
            return f"SHL V{x:X}"
    if (op & 0xF000) == 0x9000 and n == 0:
        return f"SNE V{x:X}, V{y:X}"
    if (op & 0xF000) == 0xA000:
        return f"LD I, {nnn:#05x}"
    if (op & 0xF000) == 0xB000:
        return f"JP V0, {nnn:#05x}"
    if (op & 0xF000) == 0xC000:
        return f"RND V{x:X}, {kk:#04x}"
    if (op & 0xF000) == 0xD000:
        return f"DRW V{x:X}, V{y:X}, {n}"
    if (op & 0xF0FF) == 0xE09E:
        return f"SKP V{x:X}"
    if (op & 0xF0FF) == 0xE0A1:
        return f"SKNP V{x:X}"
    if (op & 0xF0FF) == 0xF007:
        return f"LD V{x:X}, DT"
    if (op & 0xF0FF) == 0xF00A:
        return f"LD V{x:X}, K"
    if (op & 0xF0FF) == 0xF015:
        return f"LD DT, V{x:X}"
    if (op & 0xF0FF) == 0xF018:
        return f"LD ST, V{x:X}"
    if (op & 0xF0FF) == 0xF01E:
        return f"ADD I, V{x:X}"
    if (op & 0xF0FF) == 0xF029:
        return f"LD F, V{x:X}"
    if (op & 0xF0FF) == 0xF033:
        return f"LD B, V{x:X}"
    if (op & 0xF0FF) == 0xF055:
        return f"LD [I], V{x:X}"
    if (op & 0xF0FF) == 0xF065:
        return f"LD V{x:X}, [I]"
    return f"OP {op:04x}"


def box_top() -> str:
    return BOX["tl"] + (BOX["h"] * WIDTH) + BOX["tr"]


def box_bottom() -> str:
    return BOX["bl"] + (BOX["h"] * WIDTH) + BOX["br"]


def box_text(text: str) -> str:
    return BOX["v"] + (" " + text).ljust(WIDTH) + BOX["v"]


def box_title(title: str) -> str:
    title = f" {title} "
    rem = WIDTH - len(title)
    left = rem // 2
    right = rem - left
    return BOX["tl"] + (BOX["h"] * left) + title + (BOX["h"] * right) + BOX["tr"]


def double_box_block(lines: list[str]) -> list[str]:
    top = BOX["dtl"] + (BOX["dh"] * WIDTH) + BOX["dtr"]
    bottom = BOX["dbl"] + (BOX["dh"] * WIDTH) + BOX["dbr"]
    mid = [BOX["dv"] + (" " + line).ljust(WIDTH) + BOX["dv"] for line in lines]
    return [top, *mid, bottom]


def title_block(title: str, lines: list[str] | None = None) -> list[str]:
    lines = lines or []
    return [box_title(title), *[box_text(line) for line in lines], box_bottom()]


def header_block() -> list[str]:
    return [
        box_top(),
        box_text("CHIP-8 CLI ROM \u2014 xx living diagram"),
        box_text("Source: roms/cli/cli.c8s + roms/cli/lib/sys.c8s + roms/cli/lib/data.c8s"),
        box_text("Build:  roms/cli/build.sh \u2192 roms/cli/build/cli.ch8"),
        box_text("This file is byte-for-byte identical to cli.ch8 when assembled by xx.py."),
        box_bottom(),
        "",
        *double_box_block(
            [
                "Legend",
                "1NNN  JP addr            2NNN  CALL addr        ANNN  LD I, addr",
                "6XNN  LD Vx, byte        7XNN  ADD Vx, byte     8XY0 LD Vx, Vy",
                "3XNN  SE Vx, byte        4XNN  SNE Vx, byte     9XY0 SNE Vx, Vy",
                "FX55  LD [I], Vx         FX65  LD Vx, [I]      0NNN SYS (syscall)",
            ]
        ),
        "",
        *title_block(
            "Syscall Table (0x01xx)",
            [
                "0x0101 spawn  0x0102 exit   0x0103 wait   0x0104 yield",
                "0x0110 write  0x0111 read   0x0112 input_mode 0x0113 console_mode",
                "0x0120 fs_list 0x0121 fs_open 0x0122 fs_read 0x0123 fs_close",
            ],
        ),
        "",
        *title_block(
            "Memory Map (ROM image)",
            [
                "0x0200..0x0594  code (boot, repl, commands, helpers, syscalls)",
                "0x0596..0x07FF  padding (00)",
                "0x0800..0x0BE1  data (buffers + strings)",
            ],
        ),
        "",
    ]


def string_line(addr: int, name: str, s: str) -> str:
    if any(ch in s for ch in COMMENT_CHARS):
        hex_bytes = " ".join(f"{b:02x}" for b in materialize(s))
        return f"{hex_bytes} # 0x{addr:04x}: {name} (hex)"
    return f"\"{s}\" # 0x{addr:04x}: {name}"


def generate_xx(rom: bytes) -> list[str]:
    markers = {
        0x0200: title_block(
            "Boot / Device Setup",
            ["Set console mode + input mode, print welcome banner."],
        ),
        0x0210: title_block(
            "REPL Loop",
            ["prompt \u2192 read_line \u2192 tokenize \u2192 dispatch"],
        ),
        0x022c: title_block(
            "Tokenizer",
            ["Walk LINE_BUF, split tok1/tok2, trim spaces, compute lengths."],
        ),
        0x0288: title_block(
            "Dispatch (by len)",
            ["tok1 length \u2192 cmd_len2 / cmd_len3 / cmd_len4"],
        ),
        0x029c: title_block('cmd_len2 ("ls")'),
        0x02b8: title_block('cmd_len3 ("run" / "cat")'),
        0x030c: title_block('cmd_len4 ("help" / "exit")'),
        0x037c: title_block("cmd_help"),
        0x0380: title_block(
            "cmd_exit",
            ["Calls sys_exit then halts in a tight jump loop."],
        ),
        0x0388: title_block(
            "cmd_ls",
            ["Lists directory entries using sys_fs_list."],
        ),
        0x03d8: title_block(
            "cmd_run",
            ["Spawns ROM via sys_spawn + waits."],
        ),
        0x03fc: title_block(
            "cmd_cat",
            ["Open \u2192 read \u2192 write \u2192 close loop."],
        ),
        0x043c: title_block("Error + Usage Paths"),
        0x0450: title_block(
            "Print Helpers",
            ["Each helper loads a string slice and calls sys_write."],
        ),
        0x04bc: title_block(
            "read_line",
            ["Reads from stdin into LINE_BUF using sys_read."],
        ),
        0x04c8: title_block(
            "Syscall Frame Builders",
            ["frame1..frame4 write argc+args into FRAME buffer."],
        ),
        0x0538: title_block(
            "Syscall Wrappers",
            ["sys_* wrappers issue 0x01xx SYS opcodes."],
        ),
        0x0596: title_block(
            "Padding (00)",
            ["Zero fill up to data region at 0x0800."],
        ),
        0x0800: title_block(
            "Data Region",
            ["Buffers + strings. Data addresses are absolute."],
        ),
        0x0B00: title_block("Text Strings"),
    }

    string_blocks = {
        0x0B00: ("PROMPT", "> "),
        0x0B10: ("WELCOME", "chip8 cli ready\\n"),
        0x0B40: (
            "HELP_TEXT",
            "help - show commands\\nls   - list files\\nrun  - run a rom\\n"
            "cat  - print file\\nexit - quit\\n",
        ),
        0x0BA0: ("ERR_UNKNOWN", "unknown command\\n"),
        0x0BB0: ("ERR_USAGE_RUN", "usage: run <rom>\\n"),
        0x0BC1: ("ERR_USAGE_CAT", "usage: cat <file>\\n"),
        0x0BD4: ("ERR_GENERIC", "error\\n"),
        0x0BE0: ("SLASH", "/"),
        0x0BE1: ("NEWLINE", "\\n"),
    }

    buffer_blocks = {
        0x0800: ("LINE_BUF", 0x50),
        0x0850: ("FRAME", 0x10),
        0x0900: ("DIR_BUF", 0x118),
        0x0A20: ("FILE_BUF", 0x40),
    }

    block_starts = sorted(set(buffer_blocks.keys()) | set(string_blocks.keys()))

    out: list[str] = []
    out.extend(header_block())

    # Code region
    addr = BASE_ADDR
    while addr < CODE_END:
        if addr in markers:
            out.extend(markers[addr])
        off = addr - BASE_ADDR
        if off + 1 >= len(rom):
            break
        if addr + 4 <= CODE_END:
            op1 = (rom[off] << 8) | rom[off + 1]
            op2 = (rom[off + 2] << 8) | rom[off + 3]
            bytes_str = f"{rom[off]:02x} {rom[off + 1]:02x} {rom[off + 2]:02x} {rom[off + 3]:02x}"
            comment = f"# 0x{addr:04x}: {decode(op1)} | {decode(op2)}"
            out.append(f"{bytes_str} {comment}")
            addr += 4
        else:
            op1 = (rom[off] << 8) | rom[off + 1]
            bytes_str = f"{rom[off]:02x} {rom[off + 1]:02x}"
            comment = f"# 0x{addr:04x}: {decode(op1)}"
            out.append(f"{bytes_str} {comment}")
            addr += 2

    # Padding region
    addr = CODE_END
    while addr < PAD_END:
        if addr in markers:
            out.extend(markers[addr])
        off = addr - BASE_ADDR
        chunk_len = min(16, PAD_END - addr)
        chunk = rom[off : off + chunk_len]
        bytes_str = " ".join(f"{b:02x}" for b in chunk)
        out.append(f"{bytes_str} # 0x{addr:04x}: padding")
        addr += chunk_len

    # Data region
    addr = PAD_END
    while addr < BASE_ADDR + len(rom):
        if addr in markers:
            out.extend(markers[addr])
        if addr in buffer_blocks:
            name, length = buffer_blocks[addr]
            out.append(f"# {name} @ 0x{addr:04x} ({length} bytes, zero-filled)")
            for off in range(0, length, 16):
                line_addr = addr + off
                count = min(16, length - off)
                out.append(("00 " * count).strip() + f" # 0x{line_addr:04x}")
            addr += length
            continue
        if addr in string_blocks:
            name, s = string_blocks[addr]
            out.append(string_line(addr, name, s))
            addr += len(materialize(s))
            continue
        next_blocks = [b for b in block_starts if b > addr]
        next_block = min(next_blocks) if next_blocks else BASE_ADDR + len(rom)
        chunk_len = min(16, next_block - addr)
        off = addr - BASE_ADDR
        chunk = rom[off : off + chunk_len]
        bytes_str = " ".join(f"{b:02x}" for b in chunk)
        out.append(f"{bytes_str} # 0x{addr:04x}")
        addr += chunk_len

    return out


def parse_xx_subset(text: str) -> bytes:
    out = bytearray()
    for line in text.splitlines():
        stripped = line.lstrip()
        if not stripped:
            continue
        first = stripped[0]
        if first in COMMENT_CHARS:
            continue
        if 0x2500 <= ord(first) <= 0x259F:
            continue
        if stripped.startswith("\""):
            end = stripped.find("\"", 1)
            if end == -1:
                raise ValueError("unterminated string in xx file")
            out.extend(materialize(stripped[1:end]))
            continue
        if "#" in stripped:
            stripped = stripped.split("#", 1)[0]
        tokens = stripped.split()
        for tok in tokens:
            out.append(int(tok, 16))
    return bytes(out)


def verify_round_trip(xx_path: Path, rom: bytes) -> None:
    parsed = parse_xx_subset(xx_path.read_text())
    if parsed != rom:
        raise SystemExit(
            f"xx verification failed: {xx_path} does not match ROM (len {len(parsed)} != {len(rom)})"
        )


def verify_with_xx_py(xx_path: Path, rom: bytes, xx_py: Path) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "cli_from_xx.ch8"
        subprocess.run(
            [sys.executable, str(xx_py), str(xx_path), "-o", str(out)],
            check=True,
        )
        data = out.read_bytes()
        if data != rom:
            raise SystemExit(
                f"xx.py verification failed: {out} does not match ROM (len {len(data)} != {len(rom)})"
            )


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate or verify the CLI xx diagram")
    parser.add_argument(
        "--rom",
        default="roms/cli/build/cli.ch8",
        help="ROM path to read (default: roms/cli/build/cli.ch8)",
    )
    parser.add_argument(
        "--out",
        default="roms/cli/cli.xx",
        help="xx output path (default: roms/cli/cli.xx)",
    )
    parser.add_argument(
        "--build",
        action="store_true",
        help="run roms/cli/build.sh before generating",
    )
    parser.add_argument(
        "--verify",
        action="store_true",
        help="verify that the xx file round-trips to the ROM bytes",
    )
    parser.add_argument(
        "--verify-only",
        action="store_true",
        help="skip writing; just verify the existing xx file",
    )
    parser.add_argument(
        "--xx",
        default=os.environ.get("XX_PY"),
        help="path to upstream xx.py for additional verification",
    )

    args = parser.parse_args()

    if args.build:
        subprocess.run(["roms/cli/build.sh"], check=True)

    rom_path = Path(args.rom)
    xx_path = Path(args.out)

    if not rom_path.exists():
        raise SystemExit(f"ROM not found: {rom_path}")

    rom = rom_path.read_bytes()

    if not args.verify_only:
        lines = generate_xx(rom)
        xx_path.write_text("\n".join(lines) + "\n")

    if args.verify or args.verify_only:
        verify_round_trip(xx_path, rom)
        if args.xx:
            verify_with_xx_py(xx_path, rom, Path(args.xx))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
