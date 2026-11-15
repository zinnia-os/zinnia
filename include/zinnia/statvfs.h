/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_STATVFS_H
#define __ZINNIA_STATVFS_H

#include <zinnia/types.h>

#define __ST_RDONLY   1
#define __ST_NOSUID   2
#define __ST_MANDLOCK 64

struct __statvfs {
    unsigned long f_bsize;
    unsigned long f_frsize;
    __fsblkcnt_t f_blocks;
    __fsblkcnt_t f_bfree;
    __fsblkcnt_t f_bavail;
    __fsfilcnt_t f_files;
    __fsfilcnt_t f_ffree;
    __fsfilcnt_t f_favail;
    unsigned long f_fsid;
    unsigned long f_flag;
    unsigned long f_namemax;
    char f_basetype[80];
};

#endif
