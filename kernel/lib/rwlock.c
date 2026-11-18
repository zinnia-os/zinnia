// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/cpu.h>
#include <lib/percpu.h>
#include <lib/rwlock.h>
#include <lib/sched.h>
#include <lib/turnstile.h>

constexpr uintptr_t RW_WAITERS = 1 << 0;
constexpr uintptr_t RW_WRITE_WANT = 1 << 1;
constexpr uintptr_t RW_WRITE_LOCK = 1 << 2;
constexpr uintptr_t RW_FLAGS = RW_WAITERS | RW_WRITE_WANT | RW_WRITE_LOCK;
constexpr uintptr_t RW_READ_INCR = 1 << 3;
constexpr uintptr_t RW_ANON_OWNER = 1 << 3;
constexpr size_t RW_SPIN_MAX = 2048;

static_assert(alignof(struct task) > RW_WRITE_LOCK, "Task pointers need free low bits");

static inline uintptr_t self_token() {
    struct task* current = percpu_get()->sched.current;
    return current ? (uintptr_t)current : RW_ANON_OWNER;
}

static inline uintptr_t holders(uintptr_t word) {
    return word & ~RW_FLAGS;
}

static inline uintptr_t writer_of(uintptr_t word) {
    return (word & RW_WRITE_LOCK) ? holders(word) : 0;
}

static inline bool writer_running(uintptr_t owner) {
    if (owner == RW_ANON_OWNER)
        return true;
    return atomic_load(&((struct task*)owner)->on_cpu, ATOMIC_RELAXED);
}

static inline uint8_t priority_of(struct task* task) {
    return atomic_load(&task->priority, ATOMIC_RELAXED);
}

void rwlock_init(struct rwlock* rw) {
    atomic_store(&rw->word, 0, ATOMIC_RELAXED);
}

bool rwlock_try_read_lock(struct rwlock* rw) {
    uintptr_t word = atomic_load(&rw->word, ATOMIC_RELAXED);
    while (!(word & (RW_WRITE_LOCK | RW_WRITE_WANT))) {
        if (atomic_compare_exchange_weak(&rw->word, &word, word + RW_READ_INCR, ATOMIC_ACQUIRE, ATOMIC_RELAXED))
            return true;
    }
    return false;
}

bool rwlock_try_write_lock(struct rwlock* rw) {
    uintptr_t self = self_token();
    uintptr_t word = atomic_load(&rw->word, ATOMIC_RELAXED);
    while (holders(word) == 0 && !(word & RW_WRITE_LOCK)) {
        if (atomic_compare_exchange_weak(&rw->word, &word, word | self | RW_WRITE_LOCK, ATOMIC_ACQUIRE, ATOMIC_RELAXED))
            return true;
    }
    return false;
}

static void lock_slow(struct rwlock* rw, bool write) {
    uintptr_t self = self_token();
    enum turnstile_queue queue = write ? TURNSTILE_EXCLUSIVE : TURNSTILE_SHARED;
    uintptr_t need = write ? (RW_WAITERS | RW_WRITE_WANT) : RW_WAITERS;
    uintptr_t blocked_by = write ? 0 : (RW_WRITE_LOCK | RW_WRITE_WANT);
    size_t spins = 0;
    uintptr_t word = atomic_load(&rw->word, ATOMIC_RELAXED);
    assert(writer_of(word) != self || self == RW_ANON_OWNER, "Recursive lock of rwlock %p", rw);

    while (true) {
        bool held = holders(word) != 0;
        if (write ? !held : !(word & blocked_by)) {
            uintptr_t new_word = write ? (word | self | RW_WRITE_LOCK) : (word + RW_READ_INCR);
            if (atomic_compare_exchange_weak(&rw->word, &word, new_word, ATOMIC_ACQUIRE, ATOMIC_RELAXED))
                return;
            continue;
        }

        uintptr_t owner = writer_of(word);
        if (owner && spins < RW_SPIN_MAX && writer_running(owner)) {
            spins++;
            cpu_pause();
            word = atomic_load(&rw->word, ATOMIC_RELAXED);
            continue;
        }

        if (self == RW_ANON_OWNER || owner == RW_ANON_OWNER) {
            spins = 0;
            cpu_pause();
            word = atomic_load(&rw->word, ATOMIC_RELAXED);
            continue;
        }

        if ((word & need) != need) {
            if (!atomic_compare_exchange_weak(&rw->word, &word, word | need, ATOMIC_RELAXED, ATOMIC_RELAXED))
                continue;
        }

        struct turnstile_chain* chain = turnstile_lock(rw);
        word = atomic_load(&rw->word, ATOMIC_RELAXED);
        held = holders(word) != 0;
        if (!held || (word & need) != need || (!write && !(word & blocked_by))) {
            turnstile_unlock(chain);
            spins = 0;
            continue;
        }

        turnstile_block(chain, rw, (struct task*)writer_of(word), queue);
        return;
    }
}

void rwlock_read_lock(struct rwlock* rw) {
    uintptr_t word = atomic_load(&rw->word, ATOMIC_RELAXED);
    if (!(word & (RW_WRITE_LOCK | RW_WRITE_WANT)) &&
        atomic_compare_exchange_weak(&rw->word, &word, word + RW_READ_INCR, ATOMIC_ACQUIRE, ATOMIC_RELAXED))
        return;
    lock_slow(rw, false);
}

void rwlock_write_lock(struct rwlock* rw) {
    uintptr_t expected = 0;
    if (!atomic_compare_exchange(&rw->word, &expected, self_token() | RW_WRITE_LOCK, ATOMIC_ACQUIRE, ATOMIC_RELAXED))
        lock_slow(rw, true);
}

static uintptr_t readers_to_wake(struct task* reader, struct task* writer) {
    uint8_t min = writer ? priority_of(writer) : SCHED_PRIO_MIN;
    uintptr_t count = 0;
    for (struct task* t = reader; t && priority_of(t) >= min; t = tailq_next(t, ts_link))
        count++;
    return count;
}

static void release_slow(struct rwlock* rw, bool write, uintptr_t keep) {
    struct turnstile_chain* chain = turnstile_lock(rw);
    uintptr_t word = atomic_load(&rw->word, ATOMIC_RELAXED);

    if (!write) {
        while (holders(word) > RW_READ_INCR) {
            if (atomic_compare_exchange_weak(&rw->word, &word, word - RW_READ_INCR, ATOMIC_RELEASE, ATOMIC_RELAXED)) {
                turnstile_unlock(chain);
                return;
            }
        }
    }

    bool more_writers, more_readers;
    struct task* writer = turnstile_first(chain, rw, TURNSTILE_EXCLUSIVE, &more_writers);
    struct task* reader = turnstile_first(chain, rw, TURNSTILE_SHARED, &more_readers);
    uintptr_t count = (write || !writer) ? readers_to_wake(reader, writer) : 0;

    if (count == 0 && writer && keep == 0) {
        uintptr_t new_word = (uintptr_t)writer | RW_WRITE_LOCK;
        if (more_writers || reader)
            new_word |= RW_WAITERS;
        if (more_writers)
            new_word |= RW_WRITE_WANT;
        atomic_store(&rw->word, new_word, ATOMIC_RELEASE);
        turnstile_wake(chain, rw, writer, writer);
    } else {
        uintptr_t new_word = keep + count * RW_READ_INCR;
        if (writer)
            new_word |= RW_WAITERS | RW_WRITE_WANT;
        atomic_store(&rw->word, new_word, ATOMIC_RELEASE);
        if (count == 0)
            turnstile_waive(chain, rw);
        while (count--)
            turnstile_wake(chain, rw, turnstile_first(chain, rw, TURNSTILE_SHARED, &more_readers), nullptr);
    }
    turnstile_unlock(chain);
}

void rwlock_read_unlock(struct rwlock* rw) {
    uintptr_t word = atomic_load(&rw->word, ATOMIC_RELAXED);
    while (true) {
        assert(holders(word) != 0 && !(word & RW_WRITE_LOCK), "Read unlock of rwlock %p that isn't read-held", rw);
        if (holders(word) == RW_READ_INCR && (word & RW_WAITERS)) {
            release_slow(rw, false, 0);
            return;
        }
        if (atomic_compare_exchange_weak(&rw->word, &word, word - RW_READ_INCR, ATOMIC_RELEASE, ATOMIC_RELAXED))
            return;
    }
}

void rwlock_write_unlock(struct rwlock* rw) {
    uintptr_t self = self_token();
    uintptr_t expected = self | RW_WRITE_LOCK;
    if (atomic_compare_exchange(&rw->word, &expected, 0, ATOMIC_RELEASE, ATOMIC_RELAXED))
        return;

    assert(writer_of(expected) == self, "Rwlock %p unlocked by a task that doesn't own it", rw);
    release_slow(rw, true, 0);
}

void rwlock_downgrade(struct rwlock* rw) {
    uintptr_t self = self_token();
    uintptr_t expected = self | RW_WRITE_LOCK;
    if (atomic_compare_exchange(&rw->word, &expected, RW_READ_INCR, ATOMIC_RELEASE, ATOMIC_RELAXED))
        return;

    assert(writer_of(expected) == self, "Rwlock %p downgraded by a task that doesn't own it", rw);
    release_slow(rw, true, RW_READ_INCR);
}

bool rwlock_try_upgrade(struct rwlock* rw) {
    uintptr_t word = atomic_load(&rw->word, ATOMIC_RELAXED);
    assert(holders(word) != 0 && !(word & RW_WRITE_LOCK), "Upgrade of rwlock %p that isn't read-held", rw);
    uintptr_t expected = RW_READ_INCR;
    return atomic_compare_exchange(&rw->word, &expected, self_token() | RW_WRITE_LOCK, ATOMIC_ACQUIRE, ATOMIC_RELAXED);
}
