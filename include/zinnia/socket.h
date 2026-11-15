/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_SOCKET_H
#define __ZINNIA_SOCKET_H

#include <zinnia/compiler.h>
#include <zinnia/types.h>
#include <zinnia/uio.h>

typedef __zn_u32 __socklen_t;
typedef __zn_u16 __sa_family_t;

struct __msghdr {
    void __user* msg_name;
    __socklen_t msg_namelen;
    struct __iovec __user* msg_iov;
    int msg_iovlen;
    void __user* msg_control;
    __socklen_t msg_controllen;
    int msg_flags;
};

struct __cmsghdr {
    __socklen_t cmsg_len;
    int cmsg_level;
    int cmsg_type;
};

struct __sockaddr {
    __sa_family_t sa_family;
    char sa_data[14];
};

struct __sockaddr_storage {
    __sa_family_t ss_family;
    char __padding[128 - sizeof(__sa_family_t) - sizeof(__zn_uptr)];
    __zn_uptr __alignment;
};

struct __sockaddr_un {
    __sa_family_t sun_family;
    char sun_path[108];
};

typedef __zn_u32 __in_addr_t;
typedef __zn_u16 __in_port_t;

struct __in_addr {
    __in_addr_t s_addr;
};

struct __sockaddr_in {
    __sa_family_t sin_family;
    __in_port_t sin_port;
    struct __in_addr sin_addr;
    __zn_u8 sin_zero[8];
};

struct __in6_addr {
    __zn_u8 s6_addr[16];
};

struct __sockaddr_in6 {
    __sa_family_t sin6_family;
    __in_port_t sin6_port;
    __zn_u32 sin6_flowinfo;
    struct __in6_addr sin6_addr;
    __zn_u32 sin6_scope_id;
};

struct __sockaddr_ll {
    __sa_family_t sll_family;
    __zn_u16 sll_protocol;
    __zn_i32 sll_ifindex;
    __zn_u16 sll_hatype;
    __zn_u8 sll_pkttype;
    __zn_u8 sll_halen;
    __zn_u8 sll_addr[8];
};

struct __ucred {
    __pid_t pid;
    __uid_t uid;
    __gid_t gid;
};

#define __SCM_RIGHTS      1
#define __SCM_CREDENTIALS 0x02

#define __SOCK_DGRAM     (1 << 0)
#define __SOCK_RAW       (1 << 1)
#define __SOCK_STREAM    (1 << 2)
#define __SOCK_SEQPACKET (1 << 3)
#define __SOCK_NONBLOCK  (1 << 15)
#define __SOCK_CLOEXEC   (1 << 16)
#define __SOCK_CLOFORK   (1 << 17)

#define __SOL_IP     0
#define __SOL_SOCKET 1
#define __SOL_PACKET 263

#define __SO_ACCEPTCONN    1
#define __SO_BROADCAST     2
#define __SO_DEBUG         3
#define __SO_DONTROUTE     4
#define __SO_ERROR         5
#define __SO_KEEPALIVE     6
#define __SO_LINGER        7
#define __SO_OOBINLINE     8
#define __SO_RCVBUF        9
#define __SO_RCVLOWAT      10
#define __SO_RCVTIMEO      11
#define __SO_REUSEADDR     12
#define __SO_SNDBUF        13
#define __SO_SNDLOWAT      14
#define __SO_SNDTIMEO      15
#define __SO_TYPE          16
#define __SO_SNDBUFFORCE   17
#define __SO_PEERCRED      18
#define __SO_ATTACH_FILTER 19
#define __SO_PASSCRED      20
#define __SO_RCVBUFFORCE   21
#define __SO_DETACH_FILTER 22
#define __SO_PROTOCOL      23
#define __SO_REUSEPORT     24
#define __SO_BINDTODEVICE  27
#define __SO_DOMAIN        28

#define __SOMAXCONN 128

#define __MSG_CTRUNC       0x1
#define __MSG_DONTROUTE    0x2
#define __MSG_EOR          0x4
#define __MSG_OOB          0x8
#define __MSG_NOSIGNAL     0x10
#define __MSG_PEEK         0x20
#define __MSG_TRUNC        0x40
#define __MSG_WAITALL      0x80
#define __MSG_FIN          0x200
#define __MSG_CONFIRM      0x800
#define __MSG_DONTWAIT     0x1000
#define __MSG_CMSG_CLOEXEC 0x2000
#define __MSG_MORE         0x4000

#define __AF_UNSPEC 0
#define __AF_INET   1
#define __AF_INET6  2
#define __AF_UNIX   3
#define __AF_PACKET 11

#define __IPPROTO_IP   0
#define __IPPROTO_ICMP 1
#define __IPPROTO_TCP  6
#define __IPPROTO_UDP  17
#define __IPPROTO_RAW  255

#define __TCP_NODELAY 1
#define __TCP_MAXSEG  2

#define __SHUT_RD   1
#define __SHUT_WR   2
#define __SHUT_RDWR (__SHUT_RD | __SHUT_WR)

#endif
