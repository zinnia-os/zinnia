/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_UTSNAME_H
#define __ZINNIA_UTSNAME_H

struct __utsname {
    char sysname[65];
    char nodename[65];
    char release[65];
    char version[65];
    char machine[65];
    char domainname[65];
};

#endif
