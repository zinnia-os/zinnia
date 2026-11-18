/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_SOCKIOS_H
#define __ZINNIA_SOCKIOS_H

#define SIOCATMARK       __IO('Y', 0x05)
#define SIOCADDRT        __IO('Y', 0x0B)
#define SIOCDELRT        __IO('Y', 0x0C)
#define SIOCGIFNAME      __IO('Y', 0x10)
#define SIOCGIFCONF      __IO('Y', 0x12)
#define SIOCGIFFLAGS     __IO('Y', 0x13)
#define SIOCSIFFLAGS     __IO('Y', 0x14)
#define SIOCGIFADDR      __IO('Y', 0x15)
#define SIOCSIFADDR      __IO('Y', 0x16)
#define SIOCGIFBRDADDR   __IO('Y', 0x19)
#define SIOCSIFBRDADDR   __IO('Y', 0x1A)
#define SIOCGIFNETMASK   __IO('Y', 0x1B)
#define SIOCSIFNETMASK   __IO('Y', 0x1C)
#define SIOCGIFMTU       __IO('Y', 0x21)
#define SIOCSIFMTU       __IO('Y', 0x22)
#define SIOCGIFHWADDR    __IO('Y', 0x27)
#define SIOCGIFINDEX     __IO('Y', 0x33)
#define SIOCPROTOPRIVATE __IO('Y', 0xE0)
#define SIOCDEVPRIVATE   __IO('Y', 0xF0)

#endif
