/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_SOCKIOS_H
#define __ZINNIA_SOCKIOS_H

#include <zinnia/ioctls.h>

#define SIOCATMARK       __IO(0x89, 0x05)
#define SIOCADDRT        __IO(0x89, 0x0B)
#define SIOCDELRT        __IO(0x89, 0x0C)
#define SIOCGIFNAME      __IO(0x89, 0x10)
#define SIOCGIFCONF      __IO(0x89, 0x12)
#define SIOCGIFFLAGS     __IO(0x89, 0x13)
#define SIOCSIFFLAGS     __IO(0x89, 0x14)
#define SIOCGIFADDR      __IO(0x89, 0x15)
#define SIOCSIFADDR      __IO(0x89, 0x16)
#define SIOCGIFBRDADDR   __IO(0x89, 0x19)
#define SIOCSIFBRDADDR   __IO(0x89, 0x1A)
#define SIOCGIFNETMASK   __IO(0x89, 0x1B)
#define SIOCSIFNETMASK   __IO(0x89, 0x1C)
#define SIOCGIFMTU       __IO(0x89, 0x21)
#define SIOCSIFMTU       __IO(0x89, 0x22)
#define SIOCGIFHWADDR    __IO(0x89, 0x27)
#define SIOCGIFINDEX     __IO(0x89, 0x33)
#define SIOCPROTOPRIVATE __IO(0x89, 0xE0)
#define SIOCDEVPRIVATE   __IO(0x89, 0xF0)

#endif
