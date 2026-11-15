/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_DIRENT_H
#define __ZINNIA_DIRENT_H

#include <zinnia/types.h>

#define __DT_UNKNOWN 0
#define __DT_FIFO    1
#define __DT_CHR     2
#define __DT_DIR     4
#define __DT_BLK     6
#define __DT_REG     8
#define __DT_LNK     10
#define __DT_SOCK    12
#define __DT_WHT     14

struct __dirent {
    __ino_t d_ino;
    __off_t d_off;
    __zn_u16 d_reclen;
    __zn_u8 d_type;
    char d_name[256];
};

#endif
