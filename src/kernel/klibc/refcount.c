
// SPDX-License-Identifier: MIT

#include <kernel/alloc.h>
#include <kernel/panic.h>
#include <kernel/refcount.h>
#include <stdatomic.h>

[[noreturn]]
static inline void rc_abort(struct rc* rc, int count) {
    panic("Invalid reference count %d for (struct rc *) 0x%p\n", count, rc);
}

[[noreturn]]
static inline void rc_strong_fail() {
    panic("Out of memory (allocating struct rc)\n");
}

// Create a new refcount pointer, return nullptr if out of memory.
// If the creation fails, it is up to the user to clean up `data`.
struct rc* rc_new(void* data, void (*cleanup)(void*)) {
    struct rc* rc = mem_alloc(sizeof(struct rc), 0);
    if (!rc) {
        return nullptr;
    }
    rc->data = data;
    rc->cleanup = cleanup;
    atomic_store_explicit(&rc->refcount, 1, memory_order_release);
    return rc;
}

// Create a new refcount pointer, abort if out of memory.
struct rc* rc_new_strong(void* data, void (*cleanup)(void*)) {
    struct rc* rc = rc_new(data, cleanup);
    if (!rc) {
        rc_strong_fail();
    }
    return rc;
}

// Take a new share from a refcount pointer.
struct rc* rc_share(struct rc* rc) {
    int prev = atomic_fetch_add_explicit(&rc->refcount, 1, memory_order_relaxed);
    if (prev <= 0 || prev == __INT_MAX__) {
        rc_abort(rc, prev);
    }
    return rc;
}

// Delete a share from a refcount pointer.
void rc_delete(struct rc* rc) {
    int prev = atomic_fetch_sub_explicit(&rc->refcount, 1, memory_order_release);
    if (prev <= 0) {
        rc_abort(rc, prev);
    } else if (prev == 1) {
        atomic_thread_fence(memory_order_acquire);
        if (rc->cleanup) {
            rc->cleanup(rc->data);
        }
        mem_free(rc);
    }
}
