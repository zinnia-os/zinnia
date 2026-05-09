use core::fmt::Display;

#[derive(Debug)]
pub enum NvmeError {
    UnsupportedPageSize,
    MmioFailed,
    MissingQueue,
    AllocationFailed,
    CommandFailed,
    Timeout,
    ControllerFailed,
}

impl Display for NvmeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            NvmeError::UnsupportedPageSize => f.write_str("The host's page size is not supported"),
            NvmeError::MmioFailed => f.write_str("Failed to perform MMIO"),
            NvmeError::MissingQueue => f.write_str("Attempted to write to a missing queue"),
            NvmeError::AllocationFailed => f.write_str("Failed to allocate enough memory"),
            NvmeError::CommandFailed => f.write_str("A command didn't complete successfully"),
            NvmeError::Timeout => f.write_str("Timed out waiting for the controller"),
            NvmeError::ControllerFailed => f.write_str("The controller reported a fatal error"),
        }
    }
}
