pub mod timing {
    use chip8_core::device::device::TimeProvider;

    #[cfg(feature = "std")]
    use std::thread;
    #[cfg(feature = "std")]
    use std::time::{Duration, Instant};

    /// Standard time provider using std::time::Instant and thread::sleep.
    ///
    /// Efficient for std targets with OS support:
    /// - elapsed_nanos(): Fast read of monotonic clock
    /// - wait_nanos(): Uses thread::sleep() for CPU-efficient waiting
    #[cfg(feature = "std")]
    pub struct StdTimeProvider {
        start: Instant,
    }

    #[cfg(feature = "std")]
    impl StdTimeProvider {
        pub fn new() -> Self {
            StdTimeProvider {
                start: Instant::now(),
            }
        }
    }

    #[cfg(feature = "std")]
    impl TimeProvider for StdTimeProvider {
        fn elapsed_nanos(&self) -> u64 {
            self.start.elapsed().as_nanos() as u64
        }

        fn reset(&mut self) {
            self.start = Instant::now();
        }

        fn wait_nanos(&self, nanos: u64) {
            if nanos > 0 {
                thread::sleep(Duration::from_nanos(nanos));
            }
        }
    }

    /// No-std time provider stub for bare-metal/QEMU targets.
    ///
    /// Future implementation will use:
    /// - ARM: PMCCNTR (Performance Monitor Cycle Counter) for elapsed_nanos
    /// - RISC-V: RDTIME/RDTIMEH for elapsed_nanos
    /// - Busy-wait loop for wait_nanos (spin on cycle counter)
    ///
    /// Current stub behavior:
    /// - elapsed_nanos(): Returns 0 (no timing)
    /// - reset(): No-op
    /// - wait_nanos(): No-op (no waiting, runs at full speed)
    #[cfg(not(feature = "std"))]
    pub struct NoStdTimeProvider {
        // Future: Add cycle counter state here
        // For ARM: u64 cycle_offset for PMCCNTR
        // For RISC-V: u64 time_offset for RDTIME
    }

    #[cfg(not(feature = "std"))]
    impl NoStdTimeProvider {
        pub fn new() -> Self {
            NoStdTimeProvider {}
        }
    }

    #[cfg(not(feature = "std"))]
    impl TimeProvider for NoStdTimeProvider {
        fn elapsed_nanos(&self) -> u64 {
            // TODO: Implement cycle counter reads
            // ARM: Read PMCCNTR, convert cycles to nanos using known CPU frequency
            // RISC-V: Read RDTIME/RDTIMEH registers
            0
        }

        fn reset(&mut self) {
            // TODO: Store current cycle count as offset
        }

        fn wait_nanos(&self, _nanos: u64) {
            // TODO: Busy-wait loop comparing cycle counts
            // For now, no-op (runs at full speed, no throttling on bare metal)
        }
    }
}
