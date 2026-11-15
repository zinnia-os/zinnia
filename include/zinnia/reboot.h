/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_REBOOT_H
#define __ZINNIA_REBOOT_H

#define __REBOOT_MAGIC 0xDEADBEEF

#define __RB_AUTOBOOT    0xdeadb007
#define __RB_HALT_SYSTEM 0xdeaddead
#define __RB_ENABLE_CAD  0xdeadcad1
#define __RB_DISABLE_CAD 0xdeadcad0
#define __RB_POWER_OFF   0xdead00ff
#define __RB_SW_SUSPEND  0xdead2222
#define __RB_KEXEC       0xdeadecec

#endif
