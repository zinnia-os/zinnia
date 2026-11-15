/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_IOCTLS_H
#define __ZINNIA_IOCTLS_H

#define __IOC_VOID  0x00000000U
#define __IOC_OUT   0x40000000U
#define __IOC_IN    0x80000000U
#define __IOC_INOUT (__IOC_IN | __IOC_OUT)

#define __IOC(inout, group, num, len) ((inout) | (((len) & 0x3FFF) << 16) | ((group) << 8) | (num))
#define __IO(group, num)              __IOC(__IOC_VOID, (group), (num), 0)
#define __IOR(group, num, type)       __IOC(__IOC_OUT, (group), (num), sizeof(type))
#define __IOW(group, num, type)       __IOC(__IOC_IN, (group), (num), sizeof(type))
#define __IOWR(group, num, type)      __IOC(__IOC_INOUT, (group), (num), sizeof(type))
#define __IOWINT(group, num)          __IOC(__IOC_VOID, (group), (num), sizeof(int))
#define __IOC_GROUP(req)              (((req) >> 8) & 0xFF)
#define __IOC_NUM(req)                ((req) & 0xFF)
#define __IOC_LEN(req)                (((req) >> 16) & 0x3FFF)

#define __FIONREAD __IO('T', 0x1B)
#define __FIONBIO  __IO('T', 0x21)

#endif
