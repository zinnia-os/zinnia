/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_NET_H
#define __ZINNIA_NET_H

#include <zinnia/socket.h>
#include <zinnia/sockios.h>
#include <zinnia/types.h>

#define __IFNAMSIZ 16

#define __IFF_UP        0x1
#define __IFF_BROADCAST 0x2
#define __IFF_LOOPBACK  0x8
#define __IFF_RUNNING   0x40
#define __IFF_MULTICAST 0x1000

#define __ARPHRD_ETHER 1

#define __RTF_UP      0x0001
#define __RTF_GATEWAY 0x0002
#define __RTF_HOST    0x0004

#define __ETH_P_ALL  0x0003
#define __ETH_P_IP   0x0800
#define __ETH_P_ARP  0x0806
#define __ETH_P_IPV6 0x86DD

#define __PACKET_HOST      0
#define __PACKET_BROADCAST 1
#define __PACKET_MULTICAST 2
#define __PACKET_OTHERHOST 3
#define __PACKET_OUTGOING  4

#define __PACKET_AUXDATA 8

struct __ifmap {
    unsigned long mem_start;
    unsigned long mem_end;
    unsigned short base_addr;
    unsigned char irq;
    unsigned char dma;
    unsigned char port;
};

struct __ifreq {
    char ifr_name[__IFNAMSIZ];
    union {
        struct __sockaddr ifr_addr;
        struct __sockaddr ifr_dstaddr;
        struct __sockaddr ifr_broadaddr;
        struct __sockaddr ifr_netmask;
        struct __sockaddr ifr_hwaddr;
        short ifr_flags;
        int ifr_ifindex;
        int ifr_mtu;
        struct __ifmap ifr_map;
        char ifr_slave[__IFNAMSIZ];
        char ifr_newname[__IFNAMSIZ];
        __zn_uptr ifr_data;
    };
};

struct __ifconf {
    int ifc_len;
    __zn_uptr ifc_buf;
};

struct __rtentry {
    unsigned long rt_pad1;
    struct __sockaddr rt_dst;
    struct __sockaddr rt_gateway;
    struct __sockaddr rt_genmask;
    unsigned short rt_flags;
    short rt_pad2;
    unsigned long rt_pad3;
    unsigned char rt_tos;
    unsigned char rt_class;
    short rt_pad4[3];
    short rt_metric;
    __zn_uptr rt_dev;
    unsigned long rt_mtu;
    unsigned long rt_window;
    unsigned short rt_irtt;
};

#endif
