# Chip-8 Opcode Map (Current Implementation + Extension Space)

This document lists the full 16-bit opcode space and marks which instructions
are implemented in this runtime. It also calls out safe areas to add extensions
without breaking classic Chip-8 ROMs.

Legend:
- Impl: implemented in src/chip8_engine.rs
- N/I : not implemented (currently falls through / default behavior)
- Ext : available for extension (use with care; see notes)

-------------------------------------------------------------------------------
Opcode Space Map (Top Nibble Grouping)
-------------------------------------------------------------------------------

```
| Group | Opcode Pattern | Meaning (Columbia spec)             | Status | Notes
|-------|----------------|-------------------------------------|--------|-------------------------------|
| 0x0   | 00E0           | CLS (clear screen)                  | Impl   |                               |
| 0x0   | 00EE           | RET (return from subroutine)        | Impl   |                               |
| 0x0   | 0nnn           | SYS addr (legacy RCA 1802 call)     | N/I    | Ignored; possible Ext space   |
| 0x1   | 1nnn           | JP addr                             | Impl   |                               |
| 0x2   | 2nnn           | CALL addr                           | Impl   |                               |
| 0x3   | 3xkk           | SE Vx, byte                         | Impl   |                               |
| 0x4   | 4xkk           | SNE Vx, byte                        | Impl   |                               |
| 0x5   | 5xy0           | SE Vx, Vy                           | Impl   | low nibble not enforced       |
| 0x6   | 6xkk           | LD Vx, byte                         | Impl   |                               |
| 0x7   | 7xkk           | ADD Vx, byte                        | Impl   |                               |
| 0x8   | 8xy0           | LD Vx, Vy                           | Impl   |                               |
| 0x8   | 8xy1           | OR Vx, Vy                           | Impl   |                               |
| 0x8   | 8xy2           | AND Vx, Vy                          | Impl   |                               |
| 0x8   | 8xy3           | XOR Vx, Vy                          | Impl   |                               |
| 0x8   | 8xy4           | ADD Vx, Vy (VF=carry)               | Impl   |                               |
| 0x8   | 8xy5           | SUB Vx, Vy (VF=NOT borrow)          | Impl   |                               |
| 0x8   | 8xy6           | SHR Vx (VF=LSB of Vx)               | Impl   | Columbia spec behavior        |
| 0x8   | 8xy7           | SUBN Vx, Vy (VF=NOT borrow)         | Impl   | strict Vy>Vx check            |
| 0x8   | 8xyE           | SHL Vx (VF=MSB of Vx)               | Impl   | Columbia spec behavior        |
| 0x8   | 8xy?           | (other)                             | N/I    | Ext space, but avoid conflict |
| 0x9   | 9xy0           | SNE Vx, Vy                          | Impl   | low nibble not enforced       |
| 0xA   | Annn           | LD I, addr                          | Impl   |                               |
| 0xB   | Bnnn           | JP V0, addr                         | Impl   | classic behavior              |
| 0xC   | Cxkk           | RND Vx, byte                        | Impl   |                               |
| 0xD   | Dxyn           | DRW Vx, Vy, nibble                  | Impl   | 64x32 only                    |
| 0xE   | Ex9E           | SKP Vx                              | Impl   | key down                      |
| 0xE   | ExA1           | SKNP Vx                             | Impl   | key up                        |
| 0xE   | Ex??           | (other)                             | N/I    | Ext space                     |
| 0xF   | Fx07           | LD Vx, DT                           | Impl   |                               |
| 0xF   | Fx0A           | LD Vx, K (wait for key)             | Impl   | uses last_key                 |
| 0xF   | Fx15           | LD DT, Vx                           | Impl   |                               |
| 0xF   | Fx18           | LD ST, Vx                           | Impl   |                               |
| 0xF   | Fx1E           | ADD I, Vx                           | Impl   |                               |
| 0xF   | Fx29           | LD F, Vx (sprite addr)              | Impl   | page-relative I               |
| 0xF   | Fx33           | LD B, Vx (BCD)                      | Impl   | page-relative memory          |
| 0xF   | Fx55           | LD [I], V0..Vx                      | Impl   | I increments (spec)           |
| 0xF   | Fx65           | LD V0..Vx, [I]                      | Impl   | I increments (spec)           |
| 0xF   | Fx??           | (other)                             | N/I    | Large extension surface       |
```

-------------------------------------------------------------------------------
Recommended Extension Spaces (Low Collision Risk)
-------------------------------------------------------------------------------

If you want to add OS-like features, use opcode patterns that are:
- unused by classic Chip-8 ROMs
- not already occupied by common SCHIP/XO-CHIP extensions
- explicitly documented so ROMs can target the extension safely

Good options:

1) **Dedicated TRAP opcode**
   - Example: `0x00FD` or `0x0FFF` (documented as "SYS/TRAP")
   - Behavior: dispatch to a host syscall handler (V0 = syscall ID, V1.. args)
   - Advantage: minimal changes, easy to reason about

2) **Reserved Fx?? space**
   - Many Fx** values are unused in classic Chip-8
   - Avoid known SCHIP/XO-CHIP opcodes if you plan to support them later

3) **0x0nnn SYS space**
   - Classic interpreters ignore SYS; you can repurpose for OS calls
   - Consider reserving a subrange (e.g., 0x0F00-0x0FFF)

Notes on avoiding conflicts:
- SCHIP uses 00FB/00FC/00FD/00FE/00FF for scrolling and mode switching.
- XO-CHIP adds audio and extended graphics opcodes.
- If those are future goals, avoid those patterns for your OS calls.

-------------------------------------------------------------------------------
Suggested Extension Table (Example)
-------------------------------------------------------------------------------

```
| Opcode      | Proposed Meaning             | Notes
|-------------|------------------------------|-----------------------------|
| 00FF        | TRAP (syscall)               | If not using SCHIP 00FF     |
| Fx90..Fx9F  | SYS subcalls (OS service)    | Keep contiguous for clarity |
| 0x0F00..0x0FFF | SYS nnn (OS call)         | Alternative to TRAP         |
```

Pick one extension style and document it in `EXTENSION.md` so ROMs can target
it consistently.
