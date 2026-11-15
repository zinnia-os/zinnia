/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_STAT_H
#define __ZINNIA_STAT_H

#include <zinnia/time.h>
#include <zinnia/types.h>

#define __S_IFMT   0x0F000
#define __S_IFBLK  0x06000
#define __S_IFCHR  0x02000
#define __S_IFIFO  0x01000
#define __S_IFREG  0x08000
#define __S_IFDIR  0x04000
#define __S_IFLNK  0x0A000
#define __S_IFSOCK 0x0C000
#define __S_IRWXU  0700
#define __S_IRUSR  0400
#define __S_IWUSR  0200
#define __S_IXUSR  0100
#define __S_IRWXG  070
#define __S_IRGRP  040
#define __S_IWGRP  020
#define __S_IXGRP  010
#define __S_IRWXO  07
#define __S_IROTH  04
#define __S_IWOTH  02
#define __S_IXOTH  01
#define __S_ISUID  04000
#define __S_ISGID  02000
#define __S_ISVTX  01000
#define __S_IREAD  __S_IRUSR
#define __S_IWRITE __S_IWUSR
#define __S_IEXEC  __S_IXUSR

#define __UTIME_NOW  ((1l << 30) - 1l)
#define __UTIME_OMIT ((1l << 30) - 2l)

struct __stat {
    __dev_t st_dev;
    __ino_t st_ino;
    __mode_t st_mode;
    __nlink_t st_nlink;
    __uid_t st_uid;
    __gid_t st_gid;
    __dev_t st_rdev;
    __off_t st_size;
    struct __timespec st_atim;
    struct __timespec st_mtim;
    struct __timespec st_ctim;
    __blksize_t st_blksize;
    __blkcnt_t st_blocks;
};

#endif
