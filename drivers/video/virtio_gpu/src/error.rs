use core::fmt::Display;
use zinnia::posix::errno::Errno;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuError {
    AllocationFailed,
    EncodingFailed,
    QueueFull,
    NotifyFailed,
    Timeout,
    ShortResponse,
    NoScanouts,
    UnsupportedFormat,
    UnknownResource,
    NoRenderContext,
    NotAccelerated,
    Unspecified,
    OutOfHostMemory,
    InvalidScanoutId,
    InvalidResourceId,
    InvalidContextId,
    InvalidParameter,
    UnexpectedResponse(u32),
}

impl GpuError {
    pub fn from_response(type_: u32) -> Self {
        match type_ {
            crate::spec::resp::ERR_UNSPEC => GpuError::Unspecified,
            crate::spec::resp::ERR_OUT_OF_MEMORY => GpuError::OutOfHostMemory,
            crate::spec::resp::ERR_INVALID_SCANOUT_ID => GpuError::InvalidScanoutId,
            crate::spec::resp::ERR_INVALID_RESOURCE_ID => GpuError::InvalidResourceId,
            crate::spec::resp::ERR_INVALID_CONTEXT_ID => GpuError::InvalidContextId,
            crate::spec::resp::ERR_INVALID_PARAMETER => GpuError::InvalidParameter,
            other => GpuError::UnexpectedResponse(other),
        }
    }
}

impl From<GpuError> for Errno {
    fn from(value: GpuError) -> Self {
        match value {
            GpuError::AllocationFailed | GpuError::OutOfHostMemory => Errno::ENOMEM,
            GpuError::QueueFull => Errno::EBUSY,
            GpuError::Timeout => Errno::ETIMEDOUT,
            GpuError::NoScanouts | GpuError::NotAccelerated => Errno::ENODEV,
            GpuError::UnsupportedFormat
            | GpuError::InvalidScanoutId
            | GpuError::InvalidResourceId
            | GpuError::InvalidContextId
            | GpuError::InvalidParameter
            | GpuError::UnknownResource => Errno::EINVAL,
            GpuError::NoRenderContext => Errno::ENOTTY,
            _ => Errno::EIO,
        }
    }
}

impl Display for GpuError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            GpuError::AllocationFailed => f.write_str("Failed to allocate DMA memory"),
            GpuError::EncodingFailed => f.write_str("A command did not fit into its buffer"),
            GpuError::QueueFull => f.write_str("The control queue ran out of descriptors"),
            GpuError::NotifyFailed => f.write_str("Failed to notify the device"),
            GpuError::Timeout => f.write_str("Timed out waiting for the device"),
            GpuError::ShortResponse => f.write_str("The device returned a truncated response"),
            GpuError::NoScanouts => f.write_str("The device reported no enabled scanouts"),
            GpuError::UnsupportedFormat => f.write_str("The requested pixel format is unsupported"),
            GpuError::UnknownResource => f.write_str("No resource is bound to that handle"),
            GpuError::NoRenderContext => f.write_str("This file has no 3D rendering context"),
            GpuError::NotAccelerated => f.write_str("The device does not support virgl"),
            GpuError::Unspecified => f.write_str("The device reported an unspecified error"),
            GpuError::OutOfHostMemory => f.write_str("The host ran out of memory"),
            GpuError::InvalidScanoutId => f.write_str("The device rejected the scanout id"),
            GpuError::InvalidResourceId => f.write_str("The device rejected the resource id"),
            GpuError::InvalidContextId => f.write_str("The device rejected the context id"),
            GpuError::InvalidParameter => f.write_str("The device rejected a parameter"),
            GpuError::UnexpectedResponse(x) => write!(f, "Unexpected response type {x:#x}"),
        }
    }
}
