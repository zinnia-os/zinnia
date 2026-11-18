// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/cpu.h>
#include <lib/irq.h>
#include <lib/percpu.h>
#include <lib/sched.h>
#include <lib/sched_private.h>
#include <lib/spin.h>
#include <lib/turnstile.h>
#include <lib/utils.h>
#include <mem/alloc.h>

constexpr size_t TURNSTILE_CHAIN_BITS = 7;
constexpr size_t TURNSTILE_CHAINS = (1 << TURNSTILE_CHAIN_BITS);

struct turnstile_chain {
    // Aligned to cache line to avoid false sharing.
    alignas(CPU_CACHE_LINE_SIZE) struct spinlock lock;
    struct turnstile* head;
};

static struct turnstile_chain chains[TURNSTILE_CHAINS];
static struct spinlock loser_lock;

// Gets the chain from a lock object address.
static struct turnstile_chain* chain_for(const void* obj) {
    uint64_t hash = ((uintptr_t)obj >> 3) * 0x9E3779B97F4A7C15ull;
    return &chains[hash >> (64 - TURNSTILE_CHAIN_BITS)];
}

static struct turnstile* chain_lookup(struct turnstile_chain* chain, const void* obj) {
    struct turnstile* ts = chain->head;
    while (ts && ts->obj != obj)
        ts = ts->chain_next;
    return ts;
}

static void chain_remove(struct turnstile_chain* chain, struct turnstile* ts) {
    struct turnstile** link = &chain->head;
    while (*link != ts)
        link = &(*link)->chain_next;
    *link = ts->chain_next;
}

struct turnstile* turnstile_alloc() {
    struct turnstile* ts = mem_alloc(sizeof(struct turnstile), 0);
    if (ts) {
        for (size_t q = 0; q < TURNSTILE_NUM_QUEUES; q++)
            tailq_init(&ts->waiters[q]);
    }
    return ts;
}

static bool has_waiters(struct turnstile* ts) {
    for (size_t q = 0; q < TURNSTILE_NUM_QUEUES; q++) {
        if (!tailq_empty(&ts->waiters[q]))
            return true;
    }
    return false;
}

void turnstile_free(struct turnstile* ts) {
    assert(ts->obj == nullptr, "Freeing turnstile %p that is still in use", ts);
    mem_free(ts, sizeof(*ts));
}

struct turnstile_chain* turnstile_lock(const void* obj) {
    struct turnstile_chain* chain = chain_for(obj);
    spin_lock(&chain->lock);
    return chain;
}

void turnstile_unlock(struct turnstile_chain* chain) {
    spin_unlock(&chain->lock);
}

static uint8_t task_priority(struct task* task) {
    return atomic_load(&task->priority, ATOMIC_RELAXED);
}

// Inserts `task` into the waiters of `ts`, behind all waiters that are at least as urgent.
static void waiters_insert(struct turnstile* ts, struct task* task) {
    uint8_t prio = task_priority(task);
    struct task* pos = tailq_first(&ts->waiters[task->ts_queue]);
    while (pos && task_priority(pos) >= prio)
        pos = tailq_next(pos, ts_link);

    if (pos)
        tailq_insert_before(pos, task, ts_link);
    else
        tailq_insert_tail(&ts->waiters[task->ts_queue], task, ts_link);
}

// Updates the cached priority of the first waiter. Returns true if it changed.
static bool update_top(struct turnstile* ts) {
    uint8_t top = SCHED_PRIO_MIN;
    for (size_t q = 0; q < TURNSTILE_NUM_QUEUES; q++) {
        struct task* first = tailq_first(&ts->waiters[q]);
        if (first && task_priority(first) > top)
            top = task_priority(first);
    }
    return atomic_exchange(&ts->top_priority, top, ATOMIC_RELAXED) != top;
}

// Recomputes the inherited priority of `task`. Returns true if its effective priority changed.
static bool recompute_locked(struct task* task) {
    uint8_t inherited = SCHED_PRIO_MIN;
    struct turnstile* ts;
    for (ts = tailq_first(&task->pi_lenders); ts; ts = tailq_next(ts, lender_link)) {
        uint8_t top = atomic_load(&ts->top_priority, ATOMIC_RELAXED);
        if (top > inherited)
            inherited = top;
    }
    task->lent_prio = inherited;
    return sched_update_priority_locked(task);
}

// Makes the waiters of `ts` lend their priority to `owner`, and recomputes the old owner's priority.
static void set_owner(struct turnstile_chain* chain, struct turnstile* ts, struct task* owner) {
    struct task* old = ts->owner;
    if (old == owner)
        return;

    if (old && ts->lending) {
        assert(old == percpu_get()->sched.current, "Turnstile %p is owned by another task", ts);
        task_lock(old);
        tailq_remove(&old->pi_lenders, ts, lender_link);
        recompute_locked(old);
        task_unlock(old);
    }

    ts->owner = owner;
    ts->lending = false;
    if (owner) {
        assert(atomic_load(&owner->lockp, ATOMIC_RELAXED) == &chain->lock, "New owner %p isn't blocked", owner);
        tailq_insert_tail(&owner->pi_lenders, ts, lender_link);
        ts->lending = true;
        recompute_locked(owner);
    }
}

static void requeue(struct turnstile* ts, struct task* task) {
    tailq_remove(&ts->waiters[task->ts_queue], task, ts_link);
    waiters_insert(ts, task);
}

static struct spinlock* lock_owner(struct task* owner, struct spinlock* root, struct spinlock* held, bool* lost) {
    while (true) {
        struct spinlock* lock = atomic_load(&owner->lockp, ATOMIC_ACQUIRE);
        if (lock == root || lock == held)
            return nullptr;

        if (lock == &owner->lock || lock > MAX(root, held)) {
            spin_lock(lock);
        } else if (!spin_trylock(lock)) {
            *lost = true;
            return lock;
        }

        if (atomic_load(&owner->lockp, ATOMIC_RELAXED) == lock)
            return lock;
        spin_unlock(lock);
    }
}

// Recomputes the priority of the owners along the blocking chain of `blocker`.
static void propagate(struct turnstile_chain* root, struct task* blocker) {
    struct turnstile_chain* chain = root;
    struct turnstile* ts = blocker->blocked_ts;
    size_t depth = 0;
    size_t forced = 0;
    bool loser = false;
    while (ts->owner) {
        struct task* owner = ts->owner;
        assert(owner != blocker, "Deadlock: cycle in the blocking chain of task %p", blocker);
        bool lost = false;
        struct spinlock* locked = lock_owner(owner, &root->lock, &chain->lock, &lost);
        if (lost) {
            if (chain != root)
                turnstile_unlock(chain);
            turnstile_unlock(root);
            spin_lock(locked);
            spin_unlock(locked);
            if (!loser) {
                spin_lock(&loser_lock);
                loser = true;
            }
            spin_lock(&root->lock);
            chain = root;
            forced = MAX(forced, depth + 1);
            depth = 0;
            if (atomic_load(&blocker->lockp, ATOMIC_RELAXED) != &root->lock)
                break;
            ts = blocker->blocked_ts;
            continue;
        }

        if (!ts->lending) {
            tailq_insert_tail(&owner->pi_lenders, ts, lender_link);
            ts->lending = true;
        }
        bool changed = recompute_locked(owner);
        struct turnstile* next = owner->blocked_ts;
        if ((!changed && depth >= forced) || next == nullptr) {
            if (locked)
                spin_unlock(locked);
            break;
        }

        struct turnstile_chain* next_chain = chain_for(next->obj);
        assert(atomic_load(&owner->lockp, ATOMIC_RELAXED) == &next_chain->lock, "Blocked task %p isn't locked", owner);
        if (chain != root && chain != next_chain)
            turnstile_unlock(chain);
        chain = next_chain;

        requeue(next, owner);
        if (!update_top(next) && depth + 1 >= forced)
            break;

        ts = next;
        depth++;
    }
    if (chain != root)
        turnstile_unlock(chain);
    turnstile_unlock(root);
    if (loser)
        spin_unlock(&loser_lock);
}

void turnstile_block(struct turnstile_chain* chain, const void* obj, struct task* owner, enum turnstile_queue queue) {
    struct task* self = percpu_get()->sched.current;
    assert(self->turnstile != nullptr, "Task is already blocked on a turnstile");
    assert(owner != self, "Task tried to block on a lock it owns");
    irq_lock();

    // Lend our turnstile to the lock if it doesn't have one yet, otherwise donate it.
    struct turnstile* ts = chain_lookup(chain, obj);
    if (ts == nullptr) {
        ts = self->turnstile;
        ts->obj = obj;
        ts->owner = nullptr;
        ts->lending = false;
        ts->spare = nullptr;
        ts->chain_next = chain->head;
        chain->head = ts;
    } else {
        self->turnstile->spare = ts->spare;
        ts->spare = self->turnstile;
    }
    self->turnstile = nullptr;

    if (owner && ts->owner == nullptr)
        ts->owner = owner;
    assert(owner == nullptr || ts->owner == owner, "Turnstile %p has a stale owner", ts);

    spin_lock(&self->lock);
    atomic_store(&self->ts_woken, false, ATOMIC_RELAXED);
    self->blocked_ts = ts;
    self->ts_queue = queue;
    waiters_insert(ts, self);
    update_top(ts);
    atomic_store(&self->lockp, &chain->lock, ATOMIC_RELEASE);
    spin_unlock(&self->lock);

    propagate(chain, self);

    // Check if we were actually woken.
    while (!atomic_load(&self->ts_woken, ATOMIC_ACQUIRE))
        sched_block(&percpu_get()->sched);

    task_lock(self);
    task_unlock(self);
    irq_unlock();
}

struct task* turnstile_first(
    struct turnstile_chain* chain,
    const void* obj,
    enum turnstile_queue queue,
    bool* have_more
) {
    struct turnstile* ts = chain_lookup(chain, obj);
    struct task* first = ts ? tailq_first(&ts->waiters[queue]) : nullptr;
    *have_more = first && tailq_next(first, ts_link) != nullptr;
    return first;
}

void turnstile_wake(struct turnstile_chain* chain, const void* obj, struct task* task, struct task* new_owner) {
    struct turnstile* ts = task->blocked_ts;
    assert(ts && ts->obj == obj, "Task %p isn't blocked on %p", task, obj);
    assert(atomic_load(&task->lockp, ATOMIC_RELAXED) == &chain->lock, "Task %p isn't locked by %p", task, chain);

    tailq_remove(&ts->waiters[task->ts_queue], task, ts_link);

    if (!has_waiters(ts)) {
        // The last waiter takes the lock's turnstile with it.
        set_owner(chain, ts, nullptr);
        chain_remove(chain, ts);
        ts->obj = nullptr;
        task->turnstile = ts;
    } else {
        task->turnstile = ts->spare;
        ts->spare = ts->spare->spare;
        update_top(ts);
        set_owner(chain, ts, new_owner);
    }
    task->blocked_ts = nullptr;

    spin_lock(&task->lock);
    atomic_store(&task->lockp, &task->lock, ATOMIC_RELEASE);
    atomic_store(&task->ts_woken, true, ATOMIC_RELEASE);
    sched_wake_locked(task);
    spin_unlock(&task->lock);
}

void turnstile_waive(struct turnstile_chain* chain, const void* obj) {
    struct turnstile* ts = chain_lookup(chain, obj);
    if (ts)
        set_owner(chain, ts, nullptr);
}

void turnstile_reprioritize(struct task* task) {
    struct turnstile* ts = task->blocked_ts;
    if (ts == nullptr) {
        task_unlock(task);
        return;
    }

    struct turnstile_chain* chain = chain_for(ts->obj);
    requeue(ts, task);
    if (!update_top(ts)) {
        turnstile_unlock(chain);
        return;
    }
    propagate(chain, task);
}
