/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_TYPES_H
#define __ZINNIA_TYPES_H

typedef __UINT8_TYPE__ __zn_u8;
typedef __UINT16_TYPE__ __zn_u16;
typedef __UINT32_TYPE__ __zn_u32;
typedef __UINT64_TYPE__ __zn_u64;
typedef __INT8_TYPE__ __zn_i8;
typedef __INT16_TYPE__ __zn_i16;
typedef __INT32_TYPE__ __zn_i32;
typedef __INT64_TYPE__ __zn_i64;
typedef __UINTPTR_TYPE__ __zn_uptr;
typedef __INTPTR_TYPE__ __zn_iptr;
typedef __SIZE_TYPE__ __zn_size;

typedef __zn_iptr __ssize_t;
typedef __zn_i64 __off_t;
typedef __off_t __off64_t;
typedef __zn_iptr __blksize_t;
typedef __zn_iptr __blkcnt_t;
typedef __zn_uptr __clockid_t;
typedef __zn_uptr __dev_t;
typedef __zn_u32 __gid_t;
typedef __zn_uptr __ino_t;
typedef __zn_u32 __mode_t;
typedef __zn_uptr __nlink_t;
typedef __zn_i32 __pid_t;
typedef __zn_uptr __rlim_t;
typedef __zn_u32 __uid_t;
typedef __zn_uptr __fsblkcnt_t;
typedef __zn_uptr __fsfilcnt_t;

#endif
