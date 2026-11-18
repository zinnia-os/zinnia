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

#define TCGETS          __IO('T', 0x01)
#define TCSETS          __IO('T', 0x02)
#define TCSETSW         __IO('T', 0x03)
#define TCSETSF         __IO('T', 0x04)
#define TCGETA          __IO('T', 0x05)
#define TCSETA          __IO('T', 0x06)
#define TCSETAW         __IO('T', 0x07)
#define TCSETAF         __IO('T', 0x08)
#define TCSBRK          __IO('T', 0x09)
#define TCXONC          __IO('T', 0x0A)
#define TCFLSH          __IO('T', 0x0B)
#define TIOCEXCL        __IO('T', 0x0C)
#define TIOCNXCL        __IO('T', 0x0D)
#define TIOCSCTTY       __IO('T', 0x0E)
#define TIOCGPGRP       __IO('T', 0x0F)
#define TIOCSPGRP       __IO('T', 0x10)
#define TIOCOUTQ        __IO('T', 0x11)
#define TIOCSTI         __IO('T', 0x12)
#define TIOCGWINSZ      __IO('T', 0x13)
#define TIOCSWINSZ      __IO('T', 0x14)
#define TIOCMGET        __IO('T', 0x15)
#define TIOCMBIS        __IO('T', 0x16)
#define TIOCMBIC        __IO('T', 0x17)
#define TIOCMSET        __IO('T', 0x18)
#define TIOCGSOFTCAR    __IO('T', 0x19)
#define TIOCSSOFTCAR    __IO('T', 0x1A)
#define TIOCLINUX       __IO('T', 0x1C)
#define TIOCCONS        __IO('T', 0x1D)
#define TIOCGSERIAL     __IO('T', 0x1E)
#define TIOCSSERIAL     __IO('T', 0x1F)
#define TIOCPKT         __IO('T', 0x20)
#define TIOCNOTTY       __IO('T', 0x22)
#define TIOCSETD        __IO('T', 0x23)
#define TIOCGETD        __IO('T', 0x24)
#define TCSBRKP         __IO('T', 0x25)
#define TIOCSBRK        __IO('T', 0x27)
#define TIOCCBRK        __IO('T', 0x28)
#define TIOCGSID        __IO('T', 0x29)
#define TIOCGRS485      __IO('T', 0x2E)
#define TIOCSRS485      __IO('T', 0x2F)
#define TIOCGPTN        __IOR('T', 0x30, int)
#define TIOCSPTLCK      __IOW('T', 0x31, int)
#define TCGETX          __IO('T', 0x32)
#define TCSETX          __IO('T', 0x33)
#define TCSETXF         __IO('T', 0x34)
#define TCSETXW         __IO('T', 0x35)
#define TIOCSIG         __IO('T', 0x36)
#define TIOCVHANGUP     __IO('T', 0x37)
#define TIOCGPKT        __IO('T', 0x38)
#define TIOCGPTLCK      __IOR('T', 0x39, int)
#define TIOCSERCONFIG   __IO('T', 0x53)
#define TIOCSERGWILD    __IO('T', 0x54)
#define TIOCSERSWILD    __IO('T', 0x55)
#define TIOCGLCKTRMIOS  __IO('T', 0x56)
#define TIOCSLCKTRMIOS  __IO('T', 0x57)
#define TIOCSERGSTRUCT  __IO('T', 0x58)
#define TIOCSERGETLSR   __IO('T', 0x59)
#define TIOCSERGETMULTI __IO('T', 0x5A)
#define TIOCSERSETMULTI __IO('T', 0x5B)
#define TIOCMIWAIT      __IO('T', 0x5C)
#define TIOCGICOUNT     __IO('T', 0x5D)
#define TIOCGNAME       __IO('T', 0x70)

#define TIOCPKT_DATA       0
#define TIOCPKT_FLUSHREAD  1
#define TIOCPKT_FLUSHWRITE 2
#define TIOCPKT_STOP       4
#define TIOCPKT_START      8
#define TIOCPKT_NOSTOP     16
#define TIOCPKT_DOSTOP     32
#define TIOCPKT_IOCTL      64

#define TIOCSER_TEMT 0x01

#endif
