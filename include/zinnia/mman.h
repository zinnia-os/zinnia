/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_MMAN_H
#define __ZINNIA_MMAN_H

#define __PROT_NONE  0x00
#define __PROT_READ  0x01
#define __PROT_WRITE 0x02
#define __PROT_EXEC  0x04

#define __MAP_FAILED  ((void*)-1)
#define __MAP_FILE    0x00
#define __MAP_SHARED  0x01
#define __MAP_PRIVATE 0x02
#define __MAP_FIXED   0x10
#define __MAP_ANON    0x20

#define __MS_ASYNC      0x1
#define __MS_INVALIDATE 0x2
#define __MS_SYNC       0x4

#endif
