// SPDX-License-Identifier: BSD-3-Clause

#include <arch/cpu.h>
#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/mutex.h>
#include <lib/percpu.h>
#include <lib/sched.h>
#include <lib/turnstile.h>

// Set while at least one task might be blocked on the mutex.
constexpr uintptr_t MUTEX_PARKED = 1 << 0;
constexpr uintptr_t MUTEX_FLAGS = (MUTEX_PARKED);
constexpr uintptr_t MUTEX_ANON_OWNER = 1 << 1;
constexpr size_t MUTEX_SPIN_MAX = 2048;

static_assert(alignof(struct task) > MUTEX_ANON_OWNER, "Task pointers need free low bits");

static inline uintptr_t self_token() {
    struct task* current = percpu_get()->sched.current;
    return current ? (uintptr_t)current : MUTEX_ANON_OWNER;
}

static inline uintptr_t owner_of(uintptr_t word) {
    return word & ~MUTEX_FLAGS;
}

// Whether it's worth spinning for `owner` to release the lock.
static inline bool owner_is_running(uintptr_t owner) {
    if (owner == MUTEX_ANON_OWNER)
        return true; // No scheduler yet, so there is nothing to sleep on.
    return atomic_load(&((struct task*)owner)->on_cpu, ATOMIC_RELAXED);
}

void mutex_init(mutex_t* mutex) {
    atomic_store(mutex, 0, ATOMIC_RELAXED);
}

bool mutex_trylock(mutex_t* mutex) {
    uintptr_t self = self_token();
    uintptr_t word = atomic_load(mutex, ATOMIC_RELAXED);
    while (owner_of(word) == 0) {
        if (atomic_compare_exchange_weak(mutex, &word, word | self, ATOMIC_ACQUIRE, ATOMIC_RELAXED))
            return true;
    }
    return false;
}

static void mutex_lock_slow(mutex_t* mutex, uintptr_t self) {
    size_t spins = 0;
    uintptr_t word = atomic_load(mutex, ATOMIC_RELAXED);
    assert(owner_of(word) != self || self == MUTEX_ANON_OWNER, "Recursive lock of mutex %p", mutex);

    while (true) {
        uintptr_t owner = owner_of(word);

        // The mutex is free. Take it, even if others are parked.
        if (owner == 0) {
            if (atomic_compare_exchange_weak(mutex, &word, word | self, ATOMIC_ACQUIRE, ATOMIC_RELAXED))
                return;
            continue;
        }

        // The owner is running and will probably release the lock soon, so just wait for it do so.
        if (spins < MUTEX_SPIN_MAX && owner_is_running(owner)) {
            spins++;
            arch_cpu_pause();
            word = atomic_load(mutex, ATOMIC_RELAXED);
            continue;
        }

        // Before the scheduler is up, nobody can be preempted while holding a lock. Just keep spinning.
        if (self == MUTEX_ANON_OWNER || owner == MUTEX_ANON_OWNER) {
            spins = 0;
            arch_cpu_pause();
            word = atomic_load(mutex, ATOMIC_RELAXED);
            continue;
        }

        // Tell the owner that it has to wake us.
        if (!(word & MUTEX_PARKED)) {
            if (!atomic_compare_exchange_weak(mutex, &word, word | MUTEX_PARKED, ATOMIC_RELAXED, ATOMIC_RELAXED))
                continue;
        }

        // Only block if the mutex is still locked and the owner will see our flags.
        struct turnstile_chain* chain = turnstile_lock(mutex);
        word = atomic_load(mutex, ATOMIC_RELAXED);
        owner = owner_of(word);
        if (owner == 0 || !(word & MUTEX_PARKED)) {
            turnstile_unlock(chain);
            spins = 0;
            continue;
        }

        turnstile_block(chain, mutex, (struct task*)owner, TURNSTILE_EXCLUSIVE);
        spins = 0;
        word = atomic_load(mutex, ATOMIC_RELAXED);
    }
}

void mutex_lock(mutex_t* mutex) {
    uintptr_t self = self_token();
    uintptr_t expected = 0;
    if (!atomic_compare_exchange(mutex, &expected, self, ATOMIC_ACQUIRE, ATOMIC_RELAXED))
        mutex_lock_slow(mutex, self);
}

void mutex_unlock(mutex_t* mutex) {
    uintptr_t self = self_token();
    uintptr_t expected = self;
    if (atomic_compare_exchange(mutex, &expected, 0, ATOMIC_RELEASE, ATOMIC_RELAXED))
        return;

    assert(owner_of(expected) == self, "Mutex %p unlocked by a task that doesn't own it", mutex);

    struct turnstile_chain* chain = turnstile_lock(mutex);
    atomic_store(mutex, 0, ATOMIC_RELEASE);
    bool have_more;
    struct task* waiter;
    while ((waiter = turnstile_first(chain, mutex, TURNSTILE_EXCLUSIVE, &have_more)))
        turnstile_wake(chain, mutex, waiter, nullptr);
    turnstile_unlock(chain);
}
