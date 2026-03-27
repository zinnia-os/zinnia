use alloc::string::String;
use core::ffi::CStr;

pub const DT_UNKNOWN: u8 = 0;
pub const DT_FIFO: u8 = 1;
pub const DT_CHR: u8 = 2;
pub const DT_DIR: u8 = 4;
pub const DT_BLK: u8 = 6;
pub const DT_REG: u8 = 8;
pub const DT_LNK: u8 = 10;
pub const DT_SOCK: u8 = 12;
pub const DT_WHT: u8 = 14;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct dirent {
    pub d_ino: super::ino_t,
    pub d_off: super::off_t,
    pub d_reclen: u16,
    pub d_type: u8,
    pub d_name: [u8; 256],
}

impl core::fmt::Debug for dirent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("dirent")
            .field("d_ino", &self.d_ino)
            .field("d_off", &self.d_off)
            .field("d_reclen", &self.d_reclen)
            .field("d_type", &self.d_type)
            .field(
                "d_name",
                &String::from_utf8_lossy(
                    CStr::from_bytes_until_nul(&self.d_name)
                        .map_err(|_| core::fmt::Error)?
                        .to_bytes(),
                ),
            )
            .finish()
    }
}
