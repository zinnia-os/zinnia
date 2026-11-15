// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stddef.h>
#include <stdint.h>

enum atomic_order {
    ATOMIC_RELAXED = __ATOMIC_RELAXED,
    ATOMIC_ACQUIRE = __ATOMIC_ACQUIRE,
    ATOMIC_RELEASE = __ATOMIC_RELEASE,
    ATOMIC_ACQ_REL = __ATOMIC_ACQ_REL,
    ATOMIC_SEQ_CST = __ATOMIC_SEQ_CST,
};

typedef _Atomic(bool) atomic_bool;
typedef _Atomic(int) atomic_int;
typedef _Atomic(unsigned int) atomic_uint;
typedef _Atomic(long) atomic_long;
typedef _Atomic(unsigned long) atomic_ulong;
typedef _Atomic(intptr_t) atomic_intptr_t;
typedef _Atomic(uintptr_t) atomic_uintptr_t;
typedef _Atomic(size_t) atomic_size_t;

#ifdef __clang__

#define atomic_init(obj, value) __c11_atomic_init(obj, value)

#define atomic_load(obj, order)          __c11_atomic_load(obj, order)
#define atomic_store(obj, val, order)    __c11_atomic_store(obj, val, order)
#define atomic_exchange(obj, val, order) __c11_atomic_exchange(obj, val, order)

#define atomic_compare_exchange(obj, expected, desired, success, failure) \
    __c11_atomic_compare_exchange_strong(obj, expected, desired, success, failure)
#define atomic_compare_exchange_weak(obj, expected, desired, success, failure) \
    __c11_atomic_compare_exchange_weak(obj, expected, desired, success, failure)

#define atomic_fetch_add(obj, arg, order) __c11_atomic_fetch_add(obj, arg, order)
#define atomic_fetch_sub(obj, arg, order) __c11_atomic_fetch_sub(obj, arg, order)
#define atomic_fetch_and(obj, arg, order) __c11_atomic_fetch_and(obj, arg, order)
#define atomic_fetch_or(obj, arg, order)  __c11_atomic_fetch_or(obj, arg, order)
#define atomic_fetch_xor(obj, arg, order) __c11_atomic_fetch_xor(obj, arg, order)

#else

#define atomic_init(obj, value) \
    do { \
        *(obj) = (value); \
    } while (0)

#define atomic_load(obj, order)          __atomic_load_n(obj, order)
#define atomic_store(obj, val, order)    __atomic_store_n(obj, val, order)
#define atomic_exchange(obj, val, order) __atomic_exchange_n(obj, val, order)

#define atomic_compare_exchange(obj, expected, desired, success, failure) \
    __atomic_compare_exchange_n(obj, expected, desired, false, success, failure)
#define atomic_compare_exchange_weak(obj, expected, desired, success, failure) \
    __atomic_compare_exchange_n(obj, expected, desired, true, success, failure)

#define atomic_fetch_add(obj, arg, order) __atomic_fetch_add(obj, arg, order)
#define atomic_fetch_sub(obj, arg, order) __atomic_fetch_sub(obj, arg, order)
#define atomic_fetch_and(obj, arg, order) __atomic_fetch_and(obj, arg, order)
#define atomic_fetch_or(obj, arg, order)  __atomic_fetch_or(obj, arg, order)
#define atomic_fetch_xor(obj, arg, order) __atomic_fetch_xor(obj, arg, order)

#endif

#define atomic_fence(order)          __atomic_thread_fence(order)
#define atomic_compiler_fence(order) __atomic_signal_fence(order)
