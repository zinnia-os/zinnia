use core::fmt::Display;
use zinnia::posix::errno::Errno;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlkError {
    AllocationFailed,
    EncodingFailed,
    QueueFull,
    NotifyFailed,
    Timeout,
    UnsupportedLayout,
}

impl From<BlkError> for Errno {
    fn from(value: BlkError) -> Self {
        match value {
            BlkError::AllocationFailed => Errno::ENOMEM,
            BlkError::QueueFull => Errno::EBUSY,
            BlkError::Timeout => Errno::ETIMEDOUT,
            BlkError::UnsupportedLayout => Errno::ENOTSUP,
            _ => Errno::EIO,
        }
    }
}

impl Display for BlkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BlkError::AllocationFailed => f.write_str("Failed to allocate DMA memory"),
            BlkError::EncodingFailed => f.write_str("A request header did not fit into its slot"),
            BlkError::QueueFull => f.write_str("The request does not fit into the virtqueue"),
            BlkError::NotifyFailed => f.write_str("Failed to notify the device"),
            BlkError::Timeout => f.write_str("Timed out waiting for the device"),
            BlkError::UnsupportedLayout => f.write_str("The device reported an unusable geometry"),
        }
    }
}
