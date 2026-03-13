#include <kernel/futex.h>
#include <kernel/percpu.h>
#include <kernel/print.h>
#include <kernel/sched.h>
#include <kernel/spin.h>
#include <kernel/usercopy.h>
#include <uapi/errno.h>

struct futex_bucket {
    struct wait_queue wq;
    uintptr_t key; // Virtual address used as the futex key.
    bool in_use;
};

#define FUTEX_BUCKET_COUNT 64

static struct futex_bucket futex_table[FUTEX_BUCKET_COUNT];
static struct spinlock futex_table_lock;

void futex_init(void) {
    futex_table_lock = (struct spinlock){0};
    for (size_t i = 0; i < FUTEX_BUCKET_COUNT; i++) {
        wait_queue_init(&futex_table[i].wq);
        futex_table[i].key = 0;
        futex_table[i].in_use = false;
    }
}

static size_t futex_hash(uintptr_t addr) {
    // Simple hash: mix the address bits and mod by bucket count.
    addr = (addr >> 2) ^ (addr >> 12);
    return addr % FUTEX_BUCKET_COUNT;
}

static struct futex_bucket* futex_bucket_get(uintptr_t addr) {
    size_t idx = futex_hash(addr);
    // Linear probing to find a matching or free bucket.
    for (size_t i = 0; i < FUTEX_BUCKET_COUNT; i++) {
        size_t slot = (idx + i) % FUTEX_BUCKET_COUNT;
        struct futex_bucket* b = &futex_table[slot];
        if (b->in_use && b->key == addr)
            return b;
        if (!b->in_use) {
            b->key = addr;
            b->in_use = true;
            return b;
        }
    }
    return nullptr;
}

errno_t futex_wait(__user int* addr, int expected) {
    uintptr_t key = (uintptr_t)addr;

    spin_lock(&futex_table_lock);

    // Read the current value at the userspace address.
    int current;
    if (!usercopy_read(&current, addr, sizeof(int))) {
        spin_unlock(&futex_table_lock);
        return EFAULT;
    }

    // If the value has already changed, don't block.
    if (current != expected) {
        spin_unlock(&futex_table_lock);
        return 0;
    }

    struct futex_bucket* bucket = futex_bucket_get(key);
    if (!bucket) {
        spin_unlock(&futex_table_lock);
        return ENOMEM;
    }

    spin_unlock(&futex_table_lock);

    // Block on the bucket's wait queue.
    struct sched_percpu* sched = &percpu_get()->sched;
    sched_block(sched, &bucket->wq);

    return 0;
}

errno_t futex_wake(__user int* addr, int count) {
    uintptr_t key = (uintptr_t)addr;

    spin_lock(&futex_table_lock);

    size_t idx = futex_hash(key);
    struct futex_bucket* bucket = nullptr;
    for (size_t i = 0; i < FUTEX_BUCKET_COUNT; i++) {
        size_t slot = (idx + i) % FUTEX_BUCKET_COUNT;
        struct futex_bucket* b = &futex_table[slot];
        if (b->in_use && b->key == key) {
            bucket = b;
            break;
        }
        if (!b->in_use)
            break;
    }

    if (!bucket) {
        spin_unlock(&futex_table_lock);
        return 0; // No waiters, nothing to do.
    }

    spin_unlock(&futex_table_lock);

    struct sched_percpu* sched = &percpu_get()->sched;
    if (count == INT32_MAX) {
        sched_wake_all(sched, &bucket->wq);
    } else {
        for (int i = 0; i < count; i++) {
            sched_wake(sched, &bucket->wq);
        }
    }

    return 0;
}
