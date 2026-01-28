pub mod syscall {
    /// syscall handlers return scheduling outcome for the caller.
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum SyscallOutcome {
        Completed,
        Blocked,
        Yielded,
    }
}
