/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_BLOCK_H
#define __ZINNIA_BLOCK_H

#include <zinnia/ioctls.h>

#define BLKGETSIZE64 __IOR('B', 0x00, unsigned long long)
#define BLKSSZGET    __IOR('B', 0x01, unsigned int)
#define BLKRRPART    __IO('B', 0x02)

#endif
