/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_FCNTL_H
#define __ZINNIA_FCNTL_H

#include <zinnia/types.h>

#define __F_OK 0
#define __X_OK 1
#define __W_OK 2
#define __R_OK 4

#define __O_RDONLY    (1 << 0)
#define __O_WRONLY    (1 << 1)
#define __O_RDWR      (__O_RDONLY | __O_WRONLY)
#define __O_CREAT     (1 << 6)
#define __O_EXCL      (1 << 7)
#define __O_NOCTTY    (1 << 8)
#define __O_TRUNC     (1 << 9)
#define __O_APPEND    (1 << 10)
#define __O_NONBLOCK  (1 << 11)
#define __O_DSYNC     (1 << 12)
#define __O_ASYNC     (1 << 13)
#define __O_DIRECT    (1 << 14)
#define __O_LARGEFILE (1 << 15)
#define __O_DIRECTORY (1 << 16)
#define __O_NOFOLLOW  (1 << 17)
#define __O_NOATIME   (1 << 18)
#define __O_CLOEXEC   (1 << 19)
#define __O_PATH      (1 << 21)
#define __O_TMPFILE   (1 << 22)
#define __O_SYNC      (__O_DIRECTORY | __O_TMPFILE)
#define __O_RSYNC     (__O_SYNC)
#define __O_EXEC      (__O_PATH)
#define __O_SEARCH    (__O_PATH)
#define __O_ACCMODE   (__O_RDWR | __O_PATH)

#define __AT_FDCWD (-100)

#define __AT_SYMLINK_NOFOLLOW (1 << 8)
#define __AT_SYMLINK_FOLLOW   (1 << 9)
#define __AT_REMOVEDIR        (1 << 10)
#define __AT_EACCESS          (1 << 11)
#define __AT_EMPTY_PATH       (1 << 12)

#define __F_DUPFD         0
#define __F_GETFD         1
#define __F_SETFD         2
#define __F_GETFL         3
#define __F_SETFL         4
#define __F_GETLK         5
#define __F_SETLK         6
#define __F_SETLKW        7
#define __F_SETOWN        8
#define __F_GETOWN        9
#define __F_SETOWN_EX     15
#define __F_GETOWN_EX     16
#define __F_OFD_GETLK     36
#define __F_OFD_SETLK     37
#define __F_OFD_SETLKW    38
#define __F_DUPFD_CLOEXEC 1030

#define __F_RDLCK 0
#define __F_WRLCK 1
#define __F_UNLCK 2

#define __FD_CLOEXEC 1

#define __SEEK_SET 0
#define __SEEK_CUR 1
#define __SEEK_END 2

#define __POSIX_FADV_NORMAL     0
#define __POSIX_FADV_RANDOM     1
#define __POSIX_FADV_SEQUENTIAL 2
#define __POSIX_FADV_WILLNEED   3
#define __POSIX_FADV_DONTNEED   4
#define __POSIX_FADV_NOREUSE    5

struct __flock {
    __zn_i16 l_type;
    __zn_i16 l_whence;
    __off_t l_start;
    __off_t l_len;
    __pid_t l_pid;
};

#endif
