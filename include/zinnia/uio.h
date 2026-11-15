/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_UIO_H
#define __ZINNIA_UIO_H

#include <zinnia/compiler.h>
#include <zinnia/types.h>

struct __iovec {
    void __user* iov_base;
    __zn_size iov_len;
};

#endif
