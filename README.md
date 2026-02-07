# CHIP-8 Runtime

A production-ready CHIP-8 virtual machine runtime with extended syscalls, virtual memory, preemptive scheduling, and embedded target support.

## What Is This?

This project evolved from a simple CHIP-8 emulator into a **full-fledged runtime environment** that extends the CHIP-8 instruction set with modern OS primitives:

- **Process management**: Spawn, wait, exit, yield
- **Virtual memory**: Page-based address translation (4KB pages)
- **Preemptive scheduling**: Round-robin with configurable policies
- **Filesystem access**: Read/write files (feature-gated for safety)
- **I/O syscalls**: Console input/output with buffering
- **Debug interface**: Live process inspection and trace recording

**Status**: Phase 2 (Production-Ready) - 65 tests passing, comprehensive documentation, QEMU/embedded support

## Project History

### Phase 0: Manual Implementation (Emulator)
The project began as a traditional CHIP-8 emulator, hand-coded to understand the Columbia specification. Core components (opcode execution, display, input) were manually planned and implemented.

### Phase 1: Runtime Evolution (Manual)
Recognizing CHIP-8's potential as a teaching platform, the emulator was transformed into a runtime with:
- Syscall dispatcher (0x0100-0x01FF reserved range)
- Basic process abstraction
- Single-level page tables

### Phase 1.5: AI-Assisted Expansion
With the foundation solid, AI assistance (Claude Sonnet 4.5, GitHub Copilot) was used to:
- Add robustness (stack bounds, error handling, negative tests)
- Implement QEMU portability (no_std support, trait abstraction)
- Build production features (munmap, error logging, trace diagnostics)

**All AI-assisted changes followed strict principles** (see Development Philosophy below).

## AI Assistance Disclosure

### Tools Used
- **Claude Sonnet 4.5**: Architecture design, implementation, testing, documentation
- **Codex**: Architecture design, implementation, testing, documentation

### Principles
AI assistance on this project adheres to these core values (extracted from [AGENTS.md](AGENTS.md)):

1. **Understand before changing** - AI must read existing code, understand patterns, and match the established style
2. **Clarity over cleverness** - Simple, obvious code wins; no premature abstraction or over-engineering
3. **Debuggability first-class** - Error messages must be actionable; trace infrastructure required
4. **Tests as documentation** - Test names explain behavior; negative tests complement happy paths
5. **Incremental verification** - Test after each change; never batch multiple risky changes
6. **Explicit over implicit** - No magic; reader should understand code flow without consulting docs

**Human oversight**: All AI-generated code was reviewed, tested, and often refined. The human maintained architectural vision; AI executed implementation.

## Quick Start

```bash
# Clone and build
git clone https://github.com/scratchadams/chip8_runtime.git
cd chip8_runtime
cargo build --release

# Run a ROM
./target/release/chip8_runtime roms/test_opcode.ch8

# Enable syscall error logging (JSON to stderr)
CHIP8_SYSCALL_ERRORS=1 ./target/release/chip8_runtime roms/debugger.ch8 2>errors.log

# Build with filesystem write support (disabled by default)
cargo build --release --features fs_write
```

## CHIP-8 Extension Specification

This runtime extends standard CHIP-8 with syscalls in the reserved 0x0100-0x01FF range. Below is the binary format using [xx notation](https://github.com/netspooky/xx/).

### Standard CHIP-8 Opcodes (Columbia Spec)

```xx
# 0x2nnn - CALL addr (push PC, jump to nnn)
0x2 0x3 0x4 0x5   # CALL 0x345
 │   └─┴─┴─── address (12-bit): 0x345
 └─────────── opcode nibble: 0x2

# Stack grows downward from top of VM
# SP -= 2, write PC to [SP], PC = 0x345

# 0x8xy4 - ADD Vx, Vy (Vx += Vy, VF = carry)
0x8 0x1 0x2 0x4   # ADD V1, V2
 │   │   │   └─── sub-opcode: 0x4 (ADD)
 │   │   └─────── source register Y
 │   └─────────── dest register X
 └─────────────── opcode nibble: 0x8

# Columbia semantics: VF set AFTER operation
# V1 = V1 + V2, VF = (V1 + V2 > 255) ? 1 : 0
```

### Extended Syscall Format

```xx
# Syscall frame layout (stored in memory, pointed to by I register)
0x09              # frame_len: 9 bytes total
0x01 0x10         # arg0: syscall_id = 0x0110 (sys_write)
0x00 0x80         # arg1: buffer_addr = 0x0080
0x00 0x0C         # arg2: length = 12
0x00 0x00         # arg3: (unused, can extend)

# Syscall invocation opcode
0x0 0x1 0xFF      # syscall (special case of 0x0nnn)
 │   │   └─ must be 0xFF to distinguish from SYS (0x0nnn)
 │   └───── must be 0x1
 └─────────── opcode nibble: 0x0

# Execution flow:
# 1. Kernel reads frame_len from [I]
# 2. Extracts syscall_id from [I+1:I+2]
# 3. Dispatches to handler (sys_write)
# 4. Handler reads args using Kernel::syscall_arg(proc, index)
# 5. Result in V0 (bytes written), VF (error flag)
```

### Syscall Return Convention

```xx
# Success (VF = 0)
V0 = result_value   # e.g., bytes written, PID, file descriptor
VF = 0x00           # success flag

# Error (VF = 1)
V0 = error_code     # 0x02 (invalid), 0x03 (I/O), 0x04 (not found), etc.
VF = 0x01           # error flag
```

### Example: Process Spawn

```xx
# Frame in memory at 0x0200
0x0200:  0x07              # frame_len (7 bytes)
0x0201:  0x01 0x01         # syscall_id: SYS_SPAWN (0x0101)
0x0203:  0x02 0x10         # arg0: name_ptr = 0x0210 ("shell.ch8")
0x0205:  0x00 0x0A         # arg1: name_len = 10
0x0207:  0x00 0x01         # arg2: pages = 1

# ROM name at 0x0210
0x0210:  "shell.ch8"

# Invoke syscall
I = 0x0200            # Point I to frame
0x01FF                # syscall opcode

# Kernel execution:
# 1. Read frame: len=7, id=0x0101
# 2. Dispatch to sys_spawn
# 3. Read name_ptr (0x0210), name_len (10), pages (1)
# 4. Resolve "shell.ch8" relative to root dir
# 5. Allocate 1 page, create process, load ROM
# 6. Return PID in V0, VF=0

# After syscall:
V0 = 0x02             # PID = 2 (example)
VF = 0x00             # success
PC = 0x0202           # advanced past syscall opcode
```

### Memory Layout

```xx
# Process virtual address space (4KB default)
0x0000 ┌──────────────────────┐
       │  Font data (80 bytes)│
0x0050 ├──────────────────────┤
       │  Reserved            │
0x0200 ├──────────────────────┤
       │  Program ROM         │
       │  (loaded here)       │
       │                      │
       ├──────────────────────┤
       │  Heap / Stack        │
       │  (grows from top)    │
       │                      │
0x0F80 ├──────────────────────┤ ← stack_limit (default)
       │  Call stack          │
       │  (grows downward)    │
0x1000 └──────────────────────┘ ← SP starts here

# Stack frame (CALL pushes 2 bytes)
[SP-2]: PC_high
[SP-1]: PC_low
SP -= 2

# Virtual → Physical translation via page table
# Single-level: virtual_addr → page_table[page_idx] + offset
```

### Trace Record Format

```xx
# 8-byte trace record (circular buffer, readable via SYS_DBG_TRACE_READ)
0x02              # kind: TRACE_KIND_SYSCALL (0x02)
0x01              # outcome: TRACE_SYSCALL_COMPLETED (0x01)
0x00 0x01         # pid: 1 (big-endian u16)
0x01 0x10         # arg0: syscall_id = 0x0110 (sys_write)
0x00 0x02         # arg1: error_code = 0x02 (ERR_INVALID) or 0x00 (success)
 └─┴── NEW in Week 3! Previously unused, now contains error code

# Trace kinds:
# 0x01 = TRACE_KIND_SCHED (spawn, yield, preempt, block, exit, unblock)
# 0x02 = TRACE_KIND_SYSCALL (completed, yielded, blocked, error)

# Example: Failed syscall
0x02 0x01 0x00 0x01 0x01 0x10 0x00 0x02
 │    │    └──┴──   └──┴──   └──┴──
 │    │     pid=1    sys_write ERR_INVALID
 │    └── completed (with error, see VF register context)
 └────── syscall trace
```

## Documentation Directory

### Core Documentation
- **[EXTENSION.md](EXTENSION.md)**: Complete syscall reference, ABI specification, error codes
- **[SYSCALLS.md](SYSCALLS.md)**: Syscall catalog with signatures and examples
- **[ARCHITECTURE.md](ARCHITECTURE.md)**: System architecture, virtual memory, scheduling
- **[CODE_TOUR.md](CODE_TOUR.md)**: Guided tour of the codebase for new contributors

### Development Guides
- **[AGENTS.md](AGENTS.md)**: Development philosophy and AI collaboration principles
- **[QEMU_INTEGRATION.md](QEMU_INTEGRATION.md)**: Porting to bare-metal and QEMU targets
- **[SYSCALL_ERROR_LOGGING.md](SYSCALL_ERROR_LOGGING.md)**: JSON error logging format and usage
- **[TRACE_FORMAT_CHANGES.md](TRACE_FORMAT_CHANGES.md)**: Trace record format and debugger integration

### Week-by-Week Progress
- **Week 1**: Foundation & robustness (stack bounds, negative tests, documentation)
- **Week 2**: QEMU portability (no_std support, trait abstraction, allocator interface)
- **Week 3**: Production polish (munmap, error logging, filesystem writes, trace diagnostics)

See [memory/MEMORY.md](.claude/projects/-root-rust-chip8-runtime/memory/MEMORY.md) for detailed week-by-week breakdown.

## Architecture Highlights

### Virtual Memory
- **Single-level page tables**: 4KB pages, non-contiguous physical allocation
- **First-fit allocator**: Free list with automatic coalescing (Week 3)
- **Stack bounds enforcement**: Reserved 128 bytes prevents overflow/underflow

### Scheduling
- **Preemptive round-robin**: Configurable timeslice (200 steps default)
- **Pluggable policies**: `SchedulerPolicy` trait enables custom schedulers
- **Context snapshots**: Register state saved/restored across preemption

### I/O Model
- **Console modes**: Host (stdin/stdout) vs Display (GUI window)
- **Input modes**: Line-buffered vs byte-at-a-time
- **Trait-based**: `InputDevice`, `DisplayDevice`, `FsDevice` enable portability

### Safety Features
- **Feature-gated writes**: `fs_write` disabled by default, requires explicit opt-in
- **Root restriction**: All filesystem operations confined to kernel root directory
- **Path validation**: No directory traversal, no symlinks, filename length checks
- **Stack protection**: Before-check pattern prevents corruption

## Building & Testing

### Standard Build
```bash
cargo build --release          # Release build (optimized)
cargo test                      # Run all 65 tests
cargo test --test syscalls      # Run specific test suite
```

### Feature Flags
```bash
cargo build --features fs_write           # Enable filesystem write support
cargo build --no-default-features         # Minimal build (for embedded)
cargo build -p chip8_core --no-default-features  # no_std build
```

### Environment Variables
```bash
CHIP8_SYSCALL_ERRORS=1  # Enable JSON error logging to stderr
CHIP8_DEBUG_INPUT=1     # Log input events (hex + ASCII)
```

### Test Coverage
- **38 opcode tests**: Columbia semantics verification
- **2 scheduler tests**: Preemption and policy hook behavior
- **8 memory tests**: mmap/munmap, coalescing, free list
- **2 logging tests**: Error logging infrastructure
- **15 syscall tests**: Process management, I/O, filesystem, debugging

**Total: 65 tests, all passing**

## Project Structure

```
chip8_runtime/
├── src/
│   ├── main.rs                 # CLI entry point, ROM loading
│   ├── kernel.rs               # Syscall dispatcher, scheduler, process manager
│   ├── display.rs              # Display abstraction (SDL2/headless)
│   ├── proc.rs                 # Process wrapper (combines chip8_core::Proc)
│   └── shared_memory.rs        # Re-export from chip8_core
├── chip8_core/                 # Core VM (no I/O dependencies)
│   ├── src/
│   │   ├── chip8_engine.rs     # Opcode execution (Columbia spec)
│   │   ├── proc.rs             # Process state, virtual memory
│   │   ├── device.rs           # Trait definitions (DisplayDevice, etc.)
│   │   ├── shared_memory.rs    # Physical memory allocator
│   │   └── syscall.rs          # Syscall outcome types
│   └── Cargo.toml              # Features: default=["std"], no_std support
├── tests/                      # Integration tests
│   ├── opcode_semantics.rs     # CHIP-8 instruction tests
│   ├── scheduler.rs            # Preemption and policy tests
│   ├── shared_memory.rs        # Memory allocator tests
│   ├── syscall_error_logging.rs # Error logging infrastructure
│   └── syscalls.rs             # Syscall behavior tests
├── roms/                       # Test ROMs and utilities
│   ├── test_opcode.ch8         # Opcode verification ROM
│   ├── debugger.ch8            # Live debugging interface
│   └── shell.ch8               # Interactive shell
└── docs/                       # See Documentation Directory above
```

## Design Philosophy

### From AGENTS.md

**Understand before changing**
> "Read the existing code first. Understand the patterns, the style, the architecture. Match what's already there. Don't impose your own style on someone else's codebase."

**Clarity over cleverness**
> "Write code that's easy to understand, not code that makes you look smart. Future you (and future contributors) will thank you."

**Debuggability is first-class**
> "Error messages should tell you exactly what went wrong and where. Trace infrastructure isn't optional."

**Tests as documentation**
> "Test names should explain what they're testing. Reading the test suite should teach you how the system works."

**No premature abstraction**
> "Three instances of similar code is better than one overly-general abstraction that nobody understands."

### AI Collaboration Model

1. **Human sets direction**: Architecture decisions, API design, feature prioritization
2. **AI implements details**: Following patterns, writing tests, generating documentation
3. **Human reviews everything**: All AI code is tested, reviewed, and often refined
4. **Incremental verification**: Test after each change, never batch risky modifications
5. **Explicit over magic**: No hidden behavior, no surprising side effects

## Performance Characteristics

- **Syscall error logging**: ~10ns overhead when disabled (single env var check)
- **Virtual memory**: Single indirection for translation, ~5-10ns per memory access
- **Scheduler overhead**: ~2-3 clock cycles per timeslice boundary
- **munmap coalescing**: O(n) where n = number of free regions (typically < 10)

## Portability

### Supported Targets
- **Linux x86_64**: Primary development platform ✅
- **macOS ARM64**: Tested and supported ✅
- **Windows**: Planned (SDL2 dependency)
- **QEMU ARM/RISC-V**: Documented, untested (see [QEMU_INTEGRATION.md](QEMU_INTEGRATION.md))
- **Bare-metal embedded**: Trait abstraction ready, needs platform HAL

### no_std Support
```bash
# Core VM compiles without std
cargo build -p chip8_core --no-default-features

# Requires only `alloc` (Vec, Arc)
# Custom Error types for no_std
# RefCell-based Mutex for single-threaded embedded
```

## Future Work

### Planned Enhancements
- [ ] Complete syscall error logging (filesystem and debug syscalls)
- [ ] Example QEMU deployments (ARM Cortex-M, RISC-V)
- [ ] Priority scheduler and deadline scheduling
- [ ] Performance profiling syscalls (cycle counting, cache stats)
- [ ] Persistent filesystem (write-through to host)
- [ ] Network syscalls (TCP/UDP sockets)

### Research Directions
- Multi-level page tables (enable 16MB+ processes)
- Copy-on-write memory sharing between processes
- JIT compilation for hot loops
- Formal verification of syscall handlers

## Credits

**Original implementation**: Manual design and coding of emulator and runtime foundation
**AI assistance**: Claude Sonnet 4.5 (architecture, implementation, testing, docs), GitHub Copilot (code completion)
**Specification**: Columbia CHIP-8 specification (base instruction set)
**Inspiration**: xx file format notation by [netspooky](https://github.com/netspooky/xx/)

## License

[Include your license here - MIT, Apache 2.0, etc.]

## Contributing

Contributions welcome! Please:
1. Read [AGENTS.md](AGENTS.md) for development philosophy
2. Follow the established code style (clarity over cleverness)
3. Add tests for new features (integration tests in `tests/`)
4. Update documentation (in-code comments + markdown docs)
5. Run `cargo test` before submitting

For major changes, open an issue first to discuss the approach.

---

**Built with clarity, tested thoroughly, documented comprehensively.**
