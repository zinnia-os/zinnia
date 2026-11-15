/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_EPOLL_H
#define __ZINNIA_EPOLL_H

#include <zinnia/types.h>

#define __EPOLLIN        (1 << 0)
#define __EPOLLOUT       (1 << 1)
#define __EPOLLPRI       (1 << 2)
#define __EPOLLHUP       (1 << 3)
#define __EPOLLERR       (1 << 4)
#define __EPOLLRDHUP     (1 << 5)
#define __EPOLLMSG       (1 << 6)
#define __EPOLLRDNORM    (1 << 7)
#define __EPOLLRDBAND    (1 << 8)
#define __EPOLLWRNORM    (1 << 9)
#define __EPOLLWRBAND    (1 << 10)
#define __EPOLLEXCLUSIVE (1 << 11)
#define __EPOLLWAKEUP    (1 << 12)
#define __EPOLLONESHOT   (1 << 13)
#define __EPOLLET        (1 << 14)

#define __EPOLL_CTL_ADD 1
#define __EPOLL_CTL_DEL 2
#define __EPOLL_CTL_MOD 3

union __epoll_data {
    void* ptr;
    int fd;
    __zn_u32 u32;
    __zn_u64 u64;
};

struct __epoll_event {
    __zn_u32 events;
    union __epoll_data data;
};

#endif
