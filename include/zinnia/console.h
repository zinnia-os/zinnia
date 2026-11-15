/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_CONSOLE_H
#define __ZINNIA_CONSOLE_H

#include <zinnia/ioctls.h>

#define VT_AUTO    0
#define VT_PROCESS 1

#define VT_FALSE  0
#define VT_TRUE   1
#define VT_ACKACQ 2

#define KD_TEXT     0
#define KD_GRAPHICS 1

#define K_RAW   0
#define K_XLATE 1
#define K_CODE  2
#define K_OFF   4

struct vt_mode {
    char mode;
    char waitv;
    short relsig;
    short acqsig;
    short frsig;
};

struct vt_stat {
    unsigned short v_active;
    unsigned short v_signal;
    unsigned short v_state;
};

#define VT_OPENQRY    __IOR('v', 1, int)
#define VT_SETMODE    __IOW('v', 2, struct vt_mode)
#define VT_GETMODE    __IOR('v', 3, struct vt_mode)
#define VT_RELDISP    __IOWINT('v', 4)
#define VT_ACTIVATE   __IOWINT('v', 5)
#define VT_WAITACTIVE __IOWINT('v', 6)
#define VT_GETACTIVE  __IOR('v', 7, int)
#define VT_GETINDEX   __IOR('v', 8, int)
#define VT_GETSTATE   __IOR('v', 0x80, struct vt_stat)

#define KDGKBMODE __IOR('K', 6, int)
#define KDSKBMODE __IOWINT('K', 7)
#define KDGETMODE __IOR('K', 9, int)
#define KDSETMODE __IOWINT('K', 10)

typedef struct vt_mode vtmode_t;

#endif
