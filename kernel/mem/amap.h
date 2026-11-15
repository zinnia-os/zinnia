// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>
#include <lib/errno.h>
#include <lib/mutex.h>
#include <lib/radix.h>
#include <lib/refcount.h>
#include <mem/page.h>
#include <stddef.h>

struct vm_anon {
    _Atomic(size_t) rc;
    struct page* page;
};

struct amap {
    struct refcount rc;
    mutex_t lock;
    struct radix_tree anons;
};

errno_t anon_new(struct page* src, struct vm_anon** out);
void anon_ref(struct vm_anon* anon);
void anon_unref(struct vm_anon* anon);

errno_t amap_new(struct amap** out);
void amap_ref(struct amap* amap);
void amap_unref(struct amap* amap);
bool amap_is_shared(struct amap* amap);
errno_t amap_copy(struct amap* src, size_t offset, size_t num_slots, struct amap** out);
