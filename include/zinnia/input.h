/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_INPUT_H
#define __ZINNIA_INPUT_H

#include <zinnia/ioctls.h>
#include <zinnia/time.h>
#include <zinnia/types.h>

struct input_event {
    struct __timeval time;
    __zn_u16 type;
    __zn_u16 code;
    __zn_i32 value;
};

struct input_id {
    __zn_u16 bustype;
    __zn_u16 vendor;
    __zn_u16 product;
    __zn_u16 version;
};

struct input_absinfo {
    __zn_i32 value;
    __zn_i32 minimum;
    __zn_i32 maximum;
    __zn_i32 fuzz;
    __zn_i32 flat;
    __zn_i32 resolution;
};

#define EV_SYN 0x00
#define EV_KEY 0x01
#define EV_REL 0x02
#define EV_ABS 0x03
#define EV_MSC 0x04
#define EV_CNT 0x20

#define SYN_REPORT 0

#define REL_X     0x00
#define REL_Y     0x01
#define REL_WHEEL 0x08
#define REL_MAX   0x0f
#define REL_CNT   0x10

#define ABS_X        0x00
#define ABS_Y        0x01
#define ABS_Z        0x02
#define ABS_RX       0x03
#define ABS_RY       0x04
#define ABS_RZ       0x05
#define ABS_THROTTLE 0x06
#define ABS_RUDDER   0x07
#define ABS_WHEEL    0x08
#define ABS_HAT0X    0x10
#define ABS_HAT0Y    0x11
#define ABS_MAX      0x3f
#define ABS_CNT      0x40

#define KEY_RESERVED   0
#define KEY_ESC        1
#define KEY_1          2
#define KEY_2          3
#define KEY_3          4
#define KEY_4          5
#define KEY_5          6
#define KEY_6          7
#define KEY_7          8
#define KEY_8          9
#define KEY_9          10
#define KEY_0          11
#define KEY_MINUS      12
#define KEY_EQUAL      13
#define KEY_BACKSPACE  14
#define KEY_TAB        15
#define KEY_Q          16
#define KEY_W          17
#define KEY_E          18
#define KEY_R          19
#define KEY_T          20
#define KEY_Y          21
#define KEY_U          22
#define KEY_I          23
#define KEY_O          24
#define KEY_P          25
#define KEY_LEFTBRACE  26
#define KEY_RIGHTBRACE 27
#define KEY_ENTER      28
#define KEY_LEFTCTRL   29
#define KEY_A          30
#define KEY_S          31
#define KEY_D          32
#define KEY_F          33
#define KEY_G          34
#define KEY_H          35
#define KEY_J          36
#define KEY_K          37
#define KEY_L          38
#define KEY_SEMICOLON  39
#define KEY_APOSTROPHE 40
#define KEY_GRAVE      41
#define KEY_LEFTSHIFT  42
#define KEY_BACKSLASH  43
#define KEY_Z          44
#define KEY_X          45
#define KEY_C          46
#define KEY_V          47
#define KEY_B          48
#define KEY_N          49
#define KEY_M          50
#define KEY_COMMA      51
#define KEY_DOT        52
#define KEY_SLASH      53
#define KEY_RIGHTSHIFT 54
#define KEY_KPASTERISK 55
#define KEY_LEFTALT    56
#define KEY_SPACE      57
#define KEY_CAPSLOCK   58
#define KEY_F1         59
#define KEY_F2         60
#define KEY_F3         61
#define KEY_F4         62
#define KEY_F5         63
#define KEY_F6         64
#define KEY_F7         65
#define KEY_F8         66
#define KEY_F9         67
#define KEY_F10        68
#define KEY_NUMLOCK    69
#define KEY_SCROLLLOCK 70
#define KEY_KP7        71
#define KEY_KP8        72
#define KEY_KP9        73
#define KEY_KPMINUS    74
#define KEY_KP4        75
#define KEY_KP5        76
#define KEY_KP6        77
#define KEY_KPPLUS     78
#define KEY_KP1        79
#define KEY_KP2        80
#define KEY_KP3        81
#define KEY_KP0        82
#define KEY_KPDOT      83
#define KEY_F11        87
#define KEY_F12        88
#define KEY_KPENTER    96
#define KEY_RIGHTCTRL  97
#define KEY_KPSLASH    98
#define KEY_SYSRQ      99
#define KEY_RIGHTALT   100
#define KEY_HOME       102
#define KEY_UP         103
#define KEY_PAGEUP     104
#define KEY_LEFT       105
#define KEY_RIGHT      106
#define KEY_END        107
#define KEY_DOWN       108
#define KEY_PAGEDOWN   109
#define KEY_INSERT     110
#define KEY_DELETE     111
#define KEY_LEFTMETA   125
#define KEY_RIGHTMETA  126
#define KEY_F13        183
#define KEY_F14        184
#define KEY_F15        185
#define KEY_F16        186
#define KEY_F17        187
#define KEY_F18        188
#define KEY_F19        189
#define KEY_F20        190
#define KEY_F21        191
#define KEY_F22        192
#define KEY_F23        193
#define KEY_F24        194
#define KEY_CNT        0x300

#define BTN_MISC    0x100
#define BTN_LEFT    0x110
#define BTN_RIGHT   0x111
#define BTN_MIDDLE  0x112
#define BTN_SIDE    0x113
#define BTN_EXTRA   0x114
#define BTN_FORWARD 0x115
#define BTN_BACK    0x116

#define BTN_TRIGGER 0x120
#define BTN_THUMB   0x121
#define BTN_THUMB2  0x122
#define BTN_TOP     0x123
#define BTN_TOP2    0x124
#define BTN_PINKIE  0x125
#define BTN_BASE    0x126
#define BTN_BASE2   0x127
#define BTN_BASE3   0x128
#define BTN_BASE4   0x129
#define BTN_BASE5   0x12a
#define BTN_BASE6   0x12b

#define BTN_SOUTH  0x130
#define BTN_EAST   0x131
#define BTN_C      0x132
#define BTN_NORTH  0x133
#define BTN_WEST   0x134
#define BTN_Z      0x135
#define BTN_TL     0x136
#define BTN_TR     0x137
#define BTN_TL2    0x138
#define BTN_TR2    0x139
#define BTN_SELECT 0x13a
#define BTN_START  0x13b
#define BTN_MODE   0x13c
#define BTN_THUMBL 0x13d
#define BTN_THUMBR 0x13e

#define BUS_USB   0x03
#define BUS_I8042 0x11

#define EV_VERSION 0x010001

#define EVIOCGVERSION      __IOR('E', 0x01, int)
#define EVIOCGID           __IOR('E', 0x02, struct input_id)
#define EVIOCGNAME(len)    __IOC(__IOC_OUT, 'E', 0x06, len)
#define EVIOCGPHYS(len)    __IOC(__IOC_OUT, 'E', 0x07, len)
#define EVIOCGUNIQ(len)    __IOC(__IOC_OUT, 'E', 0x08, len)
#define EVIOCGPROP(len)    __IOC(__IOC_OUT, 'E', 0x09, len)
#define EVIOCGKEY(len)     __IOC(__IOC_OUT, 'E', 0x18, len)
#define EVIOCGLED(len)     __IOC(__IOC_OUT, 'E', 0x19, len)
#define EVIOCGSND(len)     __IOC(__IOC_OUT, 'E', 0x1A, len)
#define EVIOCGSW(len)      __IOC(__IOC_OUT, 'E', 0x1B, len)
#define EVIOCGBIT(ev, len) __IOC(__IOC_OUT, 'E', 0x20 + (ev), len)
#define EVIOCGABS(abs)     __IOR('E', 0x40 + (abs), struct input_absinfo)
#define EVIOCGRAB          __IOW('E', 0x90, int)
#define EVIOCREVOKE        __IOW('E', 0x91, int)
#define EVIOCSCLOCKID      __IOW('E', 0xA0, int)

#endif
