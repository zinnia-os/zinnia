#pragma once

#include <uapi/types.h>

enum open_flags {
    O_RDONLY = 1 << 0,
    O_WRONLY = 1 << 1,
    O_CREAT = 1 << 6,
    O_EXCL = 1 << 7,
    O_NOCTTY = 1 << 8,
    O_TRUNC = 1 << 9,
    O_APPEND = 1 << 10,
    O_NONBLOCK = 1 << 11,
    O_DSYNC = 1 << 12,
    O_ASYNC = 1 << 13,
    O_DIRECT = 1 << 14,
    O_LARGEFILE = 1 << 15,
    O_DIRECTORY = 1 << 16,
    O_NOFOLLOW = 1 << 17,
    O_NOATIME = 1 << 18,
    O_CLOEXEC = 1 << 19,
    O_PATH = 1 << 21,
    O_TMPFILE = 1 << 22,
    O_SYNC = O_DIRECTORY | O_TMPFILE,
    O_RSYNC = O_SYNC,

    O_EXEC = O_PATH,
    O_SEARCH = O_PATH,

    O_RDWR = O_RDONLY | O_WRONLY,
    O_ACCMODE = O_RDWR | O_PATH,
};

enum fcntl_flags {
    F_DUPFD = 0,
    F_GETFD = 1,
    F_SETFD = 2,
    F_GETFL = 3,
    F_SETFL = 4,

    F_SETOWN = 8,
    F_GETOWN = 9,
    F_SETSIG = 10,
    F_GETSIG = 11,

    F_GETLK = 5,
    F_SETLK = 6,
    F_SETLK64 = F_SETLK,
    F_SETLKW = 7,
    F_SETLKW64 = F_SETLKW,

    F_SETOWN_EX = 15,
    F_GETOWN_EX = 16,

    F_GETOWNER_UIDS = 17,

    F_SETLEASE = 1024,
    F_GETLEASE = 1025,
    F_NOTIFY = 1026,
    F_DUPFD_CLOEXEC = 1030,
    F_SETPIPE_SZ = 1031,
    F_GETPIPE_SZ = 1032,
    F_ADD_SEALS = 1033,
    F_GET_SEALS = 1034,

    F_SEAL_SEAL = 1 << 0,
    F_SEAL_SHRINK = 1 << 1,
    F_SEAL_GROW = 1 << 2,
    F_SEAL_WRITE = 1 << 3,

    F_OFD_GETLK = 36,
    F_OFD_SETLK = 37,
    F_OFD_SETLKW = 38,

    F_RDLCK = 0,
    F_WRLCK = 1,
    F_UNLCK = 2,
};

#define FD_CLOEXEC 1
#define FD_CLOFORK 2

#define AT_FDCWD (-100)

enum at_flags {
    AT_SYMLINK_NOFOLLOW = 1 << 8,
    AT_REMOVEDIR = 1 << 9,
    AT_SYMLINK_FOLLOW = 1 << 10,
    AT_EACCESS = 1 << 11,
    AT_NO_AUTOMOUNT = 1 << 12,
    AT_EMPTY_PATH = 1 << 13,
};

struct f_owner_ex {
    int32_t type;
    pid_t pid;
};

static_assert(sizeof(struct f_owner_ex) == 16);

#define F_OWNER_TID 0

enum posix_fadv {
    POSIX_FADV_NORMAL = 0,
    POSIX_FADV_RANDOM = 1,
    POSIX_FADV_SEQUENTIAL = 2,
    POSIX_FADV_WILLNEED = 3,
    POSIX_FADV_DONTNEED = 4,
    POSIX_FADV_NOREUSE = 5,
};
