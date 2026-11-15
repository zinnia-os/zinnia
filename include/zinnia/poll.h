/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_POLL_H
#define __ZINNIA_POLL_H

#include <zinnia/types.h>

#define __POLLIN     0x01
#define __POLLOUT    0x02
#define __POLLPRI    0x04
#define __POLLHUP    0x08
#define __POLLERR    0x10
#define __POLLRDHUP  0x20
#define __POLLNVAL   0x40
#define __POLLRDNORM 0x80
#define __POLLRDBAND 0x100
#define __POLLWRNORM 0x200
#define __POLLWRBAND 0x400

typedef __zn_uptr __nfds_t;

struct __pollfd {
    int fd;
    __zn_i16 events;
    __zn_i16 revents;
};

#endif
