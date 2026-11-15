/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_WAIT_H
#define __ZINNIA_WAIT_H

#define __WNOHANG    1
#define __WUNTRACED  2
#define __WSTOPPED   2
#define __WEXITED    4
#define __WCONTINUED 8
#define __WNOWAIT    0x01000000

#define __P_ALL  0
#define __P_PID  1
#define __P_PGID 2

#endif
