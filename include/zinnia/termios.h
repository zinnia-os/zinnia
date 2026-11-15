/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_TERMIOS_H
#define __ZINNIA_TERMIOS_H

#include <zinnia/ioctls.h>
#include <zinnia/types.h>

#define __NCCS 32

#define __VINTR    0
#define __VQUIT    1
#define __VERASE   2
#define __VKILL    3
#define __VEOF     4
#define __VTIME    5
#define __VMIN     6
#define __VSWTC    7
#define __VSTART   8
#define __VSTOP    9
#define __VSUSP    10
#define __VEOL     11
#define __VREPRINT 12
#define __VDISCARD 13
#define __VWERASE  14
#define __VLNEXT   15
#define __VEOL2    16

#define __IGNBRK 0000001
#define __BRKINT 0000002
#define __IGNPAR 0000004
#define __PARMRK 0000010
#define __INPCK  0000020
#define __ISTRIP 0000040
#define __INLCR  0000100
#define __IGNCR  0000200
#define __ICRNL  0000400
#define __IUCLC  0001000
#define __IXON   0002000
#define __IXANY  0004000
#define __IXOFF  0010000

#define __OPOST  0000001
#define __OLCUC  0000002
#define __ONLCR  0000004
#define __OCRNL  0000010
#define __ONOCR  0000020
#define __ONLRET 0000040

#define __CSIZE  0000060
#define __CS5    0000000
#define __CS6    0000020
#define __CS7    0000040
#define __CS8    0000060
#define __CSTOPB 0000100
#define __CREAD  0000200
#define __PARENB 0000400
#define __PARODD 0001000
#define __HUPCL  0002000
#define __CLOCAL 0004000

#define __ISIG   0000001
#define __ICANON 0000002
#define __ECHO   0000010
#define __ECHOE  0000020
#define __ECHOK  0000040
#define __ECHONL 0000100
#define __NOFLSH 0000200
#define __TOSTOP 0000400
#define __IEXTEN 0100000

#define __TCIFLUSH  0
#define __TCOFLUSH  1
#define __TCIOFLUSH 2

typedef unsigned char __cc_t;
typedef unsigned int __speed_t;
typedef unsigned int __tcflag_t;

struct __termios {
    __tcflag_t c_iflag;
    __tcflag_t c_oflag;
    __tcflag_t c_cflag;
    __tcflag_t c_lflag;
    __cc_t c_line;
    __cc_t c_cc[__NCCS];
    __speed_t c_ibaud;
    __speed_t c_obaud;
};

struct __winsize {
    unsigned short ws_row;
    unsigned short ws_col;
    unsigned short ws_xpixel;
    unsigned short ws_ypixel;
};

#define __TCGETS     __IO('T', 0x01)
#define __TCSETS     __IO('T', 0x02)
#define __TCSETSW    __IO('T', 0x03)
#define __TCSETSF    __IO('T', 0x04)
#define __TCFLSH     __IO('T', 0x0B)
#define __TIOCSCTTY  __IO('T', 0x0E)
#define __TIOCGPGRP  __IO('T', 0x0F)
#define __TIOCSPGRP  __IO('T', 0x10)
#define __TIOCGWINSZ __IO('T', 0x13)
#define __TIOCSWINSZ __IO('T', 0x14)
#define __TIOCCONS   __IO('T', 0x1D)
#define __TIOCPKT    __IO('T', 0x20)
#define __TIOCNOTTY  __IO('T', 0x22)
#define __TIOCGSID   __IO('T', 0x29)
#define __TIOCSIG    __IO('T', 0x36)
#define __TIOCGPKT   __IO('T', 0x38)
#define __TIOCGNAME  __IO('T', 0x70)
#define __TIOCGPTN   __IOR('T', 0x30, int)
#define __TIOCSPTLCK __IOW('T', 0x31, int)
#define __TIOCGPTLCK 3

#define __TIOCPKT_DATA       0
#define __TIOCPKT_FLUSHREAD  1
#define __TIOCPKT_FLUSHWRITE 2
#define __TIOCPKT_STOP       4
#define __TIOCPKT_START      8

#endif
