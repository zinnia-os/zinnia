// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>
#include <lib/epoch.h>

#define eplist_head(type) \
    struct { \
        _Atomic(type*) eph_first; \
    }

#define eplist_link(type) \
    struct { \
        _Atomic(type*) epl_next; \
        _Atomic(type*)* epl_prev; \
    }

#define eplist_init(head) atomic_init(&(head)->eph_first, nullptr)

#define eplist_first(head) epoch_deref(&(head)->eph_first)

#define eplist_next(elm, field) epoch_deref(&(elm)->field.epl_next)

#define eplist_empty(head) (atomic_load(&(head)->eph_first, ATOMIC_RELAXED) == nullptr)

#define eplist_foreach(var, head, field) for ((var) = eplist_first(head); (var); (var) = eplist_next(var, field))

#define eplist_insert_head(head, elm, field) \
    do { \
        typeof(elm) _epl_first = atomic_load(&(head)->eph_first, ATOMIC_RELAXED); \
        atomic_store(&(elm)->field.epl_next, _epl_first, ATOMIC_RELAXED); \
        (elm)->field.epl_prev = &(head)->eph_first; \
        if (_epl_first) \
            _epl_first->field.epl_prev = &(elm)->field.epl_next; \
        epoch_publish(&(head)->eph_first, (elm)); \
    } while (0)

#define eplist_insert_after(listelm, elm, field) \
    do { \
        typeof(elm) _epl_next = atomic_load(&(listelm)->field.epl_next, ATOMIC_RELAXED); \
        atomic_store(&(elm)->field.epl_next, _epl_next, ATOMIC_RELAXED); \
        (elm)->field.epl_prev = &(listelm)->field.epl_next; \
        if (_epl_next) \
            _epl_next->field.epl_prev = &(elm)->field.epl_next; \
        epoch_publish(&(listelm)->field.epl_next, (elm)); \
    } while (0)

#define eplist_insert_before(listelm, elm, field) \
    do { \
        atomic_store(&(elm)->field.epl_next, (listelm), ATOMIC_RELAXED); \
        (elm)->field.epl_prev = (listelm)->field.epl_prev; \
        epoch_publish((listelm)->field.epl_prev, (elm)); \
        (listelm)->field.epl_prev = &(elm)->field.epl_next; \
    } while (0)

#define eplist_remove(elm, field) \
    do { \
        typeof(elm) _epl_next = atomic_load(&(elm)->field.epl_next, ATOMIC_RELAXED); \
        if (_epl_next) \
            _epl_next->field.epl_prev = (elm)->field.epl_prev; \
        epoch_publish((elm)->field.epl_prev, _epl_next); \
    } while (0)
