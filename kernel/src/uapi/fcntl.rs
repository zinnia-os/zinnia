pub const O_RDONLY: u32 = 1 << 0;
pub const O_WRONLY: u32 = 1 << 1;
pub const O_CREAT: u32 = 1 << 6;
pub const O_EXCL: u32 = 1 << 7;
pub const O_NOCTTY: u32 = 1 << 8;
pub const O_TRUNC: u32 = 1 << 9;
pub const O_APPEND: u32 = 1 << 10;
pub const O_NONBLOCK: u32 = 1 << 11;
pub const O_DSYNC: u32 = 1 << 12;
pub const O_ASYNC: u32 = 1 << 13;
pub const O_DIRECT: u32 = 1 << 14;
pub const O_LARGEFILE: u32 = 1 << 15;
pub const O_DIRECTORY: u32 = 1 << 16;
pub const O_NOFOLLOW: u32 = 1 << 17;
pub const O_NOATIME: u32 = 1 << 18;
pub const O_CLOEXEC: u32 = 1 << 19;
pub const O_PATH: u32 = 1 << 21;
pub const O_TMPFILE: u32 = 1 << 22;
pub const O_SYNC: u32 = O_DIRECTORY | O_TMPFILE;
pub const O_RSYNC: u32 = O_SYNC;
pub const O_EXEC: u32 = O_PATH;
pub const O_SEARCH: u32 = O_PATH;
pub const O_RDWR: u32 = O_RDONLY | O_WRONLY;
pub const O_ACCMODE: u32 = O_RDWR | O_PATH;

pub const F_DUPFD: u32 = 0;
pub const F_GETFD: u32 = 1;
pub const F_SETFD: u32 = 2;
pub const F_GETFL: u32 = 3;
pub const F_SETFL: u32 = 4;
pub const F_SETOWN: u32 = 8;
pub const F_GETOWN: u32 = 9;
pub const F_SETSIG: u32 = 10;
pub const F_GETSIG: u32 = 11;
pub const F_GETLK: u32 = 5;
pub const F_SETLK: u32 = 6;
pub const F_SETLK64: u32 = F_SETLK;
pub const F_SETLKW: u32 = 7;
pub const F_SETLKW64: u32 = F_SETLKW;
pub const F_SETOWN_EX: u32 = 15;
pub const F_GETOWN_EX: u32 = 16;
pub const F_GETOWNER_UIDS: u32 = 17;
pub const F_SETLEASE: u32 = 1024;
pub const F_GETLEASE: u32 = 1025;
pub const F_NOTIFY: u32 = 1026;
pub const F_DUPFD_CLOEXEC: u32 = 1030;
pub const F_SETPIPE_SZ: u32 = 1031;
pub const F_GETPIPE_SZ: u32 = 1032;
pub const F_ADD_SEALS: u32 = 1033;
pub const F_GET_SEALS: u32 = 1034;
pub const F_SEAL_SEAL: u32 = 1 << 0;
pub const F_SEAL_SHRINK: u32 = 1 << 1;
pub const F_SEAL_GROW: u32 = 1 << 2;
pub const F_SEAL_WRITE: u32 = 1 << 3;
pub const F_OFD_GETLK: u32 = 36;
pub const F_OFD_SETLK: u32 = 37;
pub const F_OFD_SETLKW: u32 = 38;
pub const F_RDLCK: u32 = 0;
pub const F_WRLCK: u32 = 1;
pub const F_UNLCK: u32 = 2;

pub const FD_CLOEXEC: u32 = 1;

pub const AT_FDCWD: i32 = -100;

pub const AT_SYMLINK_NOFOLLOW: u32 = 1 << 8;
pub const AT_SYMLINK_FOLLOW: u32 = 1 << 9;
pub const AT_REMOVEDIR: u32 = 1 << 10;
pub const AT_EACCESS: u32 = 1 << 11;
pub const AT_EMPTY_PATH: u32 = 1 << 12;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct f_owner_ex {
    pub typ: i32,
    pub pid: super::pid_t,
}

pub const F_OWNER_TID: u32 = 0;
pub const POSIX_FADV_NORMAL: u32 = 0;
pub const POSIX_FADV_RANDOM: u32 = 1;
pub const POSIX_FADV_SEQUENTIAL: u32 = 2;
pub const POSIX_FADV_WILLNEED: u32 = 3;
pub const POSIX_FADV_DONTNEED: u32 = 4;
pub const POSIX_FADV_NOREUSE: u32 = 5;
