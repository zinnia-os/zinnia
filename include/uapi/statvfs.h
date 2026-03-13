#pragma once

#include <uapi/types.h>

enum statvfs_flags {
    ST_RDONLY = 1 << 0,
    ST_NOSUID = 1 << 1,
    ST_NODEV = 1 << 2,
    ST_NOEXEC = 1 << 3,
};

struct statvfs {
    uintptr_t f_bsize;
    uintptr_t f_frsize;
    fsblkcnt_t f_blocks;
    fsblkcnt_t f_bfree;
    fsblkcnt_t f_bavail;
    fsfilcnt_t f_files;
    fsfilcnt_t f_ffree;
    fsfilcnt_t f_favail;
    uintptr_t f_fsid;
    uintptr_t f_flag;
    uintptr_t f_namemax;
};
