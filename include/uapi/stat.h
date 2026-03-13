#pragma once

#include <uapi/time.h>
#include <uapi/types.h>

enum stat_flags : mode_t {
    S_IFMT = 0x0F000,
    S_IFBLK = 0x06000,
    S_IFCHR = 0x02000,
    S_IFIFO = 0x01000,
    S_IFREG = 0x08000,
    S_IFDIR = 0x04000,
    S_IFLNK = 0x0A000,
    S_IFSOCK = 0x0C000,

    S_IRWXU = 0700,
    S_IRUSR = 0400,
    S_IWUSR = 0200,
    S_IXUSR = 0100,
    S_IRWXG = 070,
    S_IRGRP = 040,
    S_IWGRP = 020,
    S_IXGRP = 010,
    S_IRWXO = 07,
    S_IROTH = 04,
    S_IWOTH = 02,
    S_IXOTH = 01,
    S_ISUID = 04000,
    S_ISGID = 02000,
    S_ISVTX = 01000,

    S_IREAD = S_IRUSR,
    S_IWRITE = S_IWUSR,
    S_IEXEC = S_IXUSR,
};

struct stat {
    dev_t st_dev;
    ino_t st_ino;
    mode_t st_mode;
    nlink_t st_nlink;
    uid_t st_uid;
    gid_t st_gid;
    dev_t st_rdev;
    off_t st_size;
    struct timespec st_atim;
    struct timespec st_mtim;
    struct timespec st_ctim;
    blksize_t st_blksize;
    blkcnt_t st_blocks;
};
