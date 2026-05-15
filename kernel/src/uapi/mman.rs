pub const PROT_NONE: u32 = 0x00;
pub const PROT_READ: u32 = 0x01;
pub const PROT_WRITE: u32 = 0x02;
pub const PROT_EXEC: u32 = 0x04;

pub const MAP_FAILED: usize = -1isize as usize;
pub const MAP_FILE: u32 = 0x00;
pub const MAP_SHARED: u32 = 0x01;
pub const MAP_PRIVATE: u32 = 0x02;
pub const MAP_FIXED: u32 = 0x10;
pub const MAP_ANON: u32 = 0x20;
pub const MAP_ANONYMOUS: u32 = 0x20;

pub const MS_ASYNC: i32 = 0x1;
pub const MS_INVALIDATE: i32 = 0x2;
pub const MS_SYNC: i32 = 0x4;
