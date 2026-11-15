/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_SCHED_H
#define __ZINNIA_SCHED_H

#define __SCHED_OTHER 0
#define __SCHED_FIFO  1
#define __SCHED_RR    2

struct __sched_param {
    int sched_priority;
    int sched_priority_min;
    int sched_priority_max;
};

#endif
