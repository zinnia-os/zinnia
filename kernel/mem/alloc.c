// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/compiler.h>
#include <lib/cpu.h>
#include <lib/mutex.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/tailq.h>
#include <lib/time.h>
#include <lib/timer.h>
#include <lib/utils.h>
#include <lib/workqueue.h>
#include <mem/alloc.h>
#include <mem/layout.h>
#include <mem/page.h>
#include <stdcountof.h>
#include <stdint.h>
#include <string.h>

constexpr size_t MEM_MAX_CACHES = 256;
constexpr size_t MEM_CACHE_NAME_MAX = 32;
constexpr size_t MEM_ALIGN_SHIFT = 4;
constexpr size_t MEM_ALIGN = 1 << MEM_ALIGN_SHIFT;

// Keep this in sync with MEM_ALLOC_SIZES[-1].
constexpr uint16_t MEM_MAXBUF = 2688;
// Object sizes for mem_alloc caches.
constexpr uint16_t MEM_ALLOC_SIZES[] = {
    16,  32,  48,  64,  80,  96,  112,  128,  160,  192,  224,  256,  320,
    384, 448, 512, 640, 768, 896, 1024, 1152, 1344, 1600, 2048, 2688,
};

struct mem_slab {
    struct mem_cache* cache;
    void* base;
    void* free;
    size_t refcount;
    size_t chunks;
    size_t num_pages;
    tailq_link(struct mem_slab) link;
};

struct mem_magazine {
    struct mem_magazine* next;
    void* rounds[];
};

struct mem_magtype {
    int16_t num_rounds;
    size_t min_obj_size;
    size_t max_obj_size;
    struct mem_cache* cache;
};

// We want each magazine to hold at most this many bytes worth of objects, so
// "next type's round count * object size" must be <= MEM_MAG_BUDGET.
constexpr size_t MEM_MAG_BUDGET = 64 * 1024;
#define MAG_MAX_OBJ_SIZE(next_rounds) (MEM_MAG_BUDGET / ((next_rounds) + 1))

// A struct mem_magazine with one round is the size of two pointers.
// From there we can derive the next size with:
//     num_rounds[i + 1] == ((num_rounds[i] + 1) * 2 - 1)
//
// min_obj_size specifies the size an object has to be greater than in order to
// be eligible for that mag type. Otherwise it falls through to the next one.
static struct mem_magtype mem_magtypes[] = {
    {.num_rounds = 1, .min_obj_size = MEM_MAXBUF, .max_obj_size = MAG_MAX_OBJ_SIZE(3)},
    {.num_rounds = 3, .min_obj_size = 256, .max_obj_size = MAG_MAX_OBJ_SIZE(7)},
    {.num_rounds = 7, .min_obj_size = 64, .max_obj_size = MAG_MAX_OBJ_SIZE(15)},
    {.num_rounds = 15, .min_obj_size = 0, .max_obj_size = MAG_MAX_OBJ_SIZE(31)},
    {.num_rounds = 31, .min_obj_size = 0, .max_obj_size = MAG_MAX_OBJ_SIZE(63)},
    {.num_rounds = 63, .min_obj_size = 0, .max_obj_size = MAG_MAX_OBJ_SIZE(127)},
    {.num_rounds = 127, .min_obj_size = 0, .max_obj_size = MAG_MAX_OBJ_SIZE(255)},
    {.num_rounds = 255, .min_obj_size = 0, .max_obj_size = 0}, // Top
};

struct mem_cpu_cache {
    alignas(CPU_CACHE_LINE_SIZE) mutex_t lock;
    struct mem_magazine* loaded;
    struct mem_magazine* loaded_prev;
    int16_t rounds;
    int16_t rounds_prev;
    int16_t num_rounds;
};
static_assert(sizeof(struct mem_cpu_cache) == CPU_CACHE_LINE_SIZE);
static_assert(alignof(struct mem_cpu_cache) == CPU_CACHE_LINE_SIZE);

struct mem_cache {
    char name[MEM_CACHE_NAME_MAX];
    size_t id;
    bool active;
    bool is_inline; // If true, the slab header is stored inside the page.
    size_t size;
    size_t chunk_size;
    size_t slab_pages;
    size_t slab_chunks;

    mutex_t lock;
    tailq_head(struct mem_slab) partial;
    size_t num_slabs;

    mutex_t depot_lock;
    struct mem_magazine* depot_full;
    struct mem_magazine* depot_empty;
    uint64_t depot_contention;
    uint64_t depot_contention_prev;
    _Atomic(struct mem_magtype*) magtype;
};

static struct mem_cache mem_caches[MEM_MAX_CACHES];
static mutex_t mem_cache_lock;
static struct mem_cache* mem_slab_cache;
static struct mem_cache* mem_alloc_table[MEM_MAXBUF >> MEM_ALIGN_SHIFT];

[[__percpu]]
static struct mem_cpu_cache mem_cpu_caches[MEM_MAX_CACHES];

// Gets the slab struct from the end of a slab page.
static inline struct mem_slab* slab_trailer(void* page) {
    return (struct mem_slab*)((uintptr_t)page + page_size() - sizeof(struct mem_slab));
}

// Finds the slab a buffer belongs to.
static inline struct mem_slab* slab_of(struct mem_cache* cache, void* buf) {
    if (!cache->is_inline)
        return page_from_addr((uintptr_t)buf - mem_hhdm_base)->obj.owner;
    return slab_trailer((void*)ROUND_DOWN((uintptr_t)buf, page_size()));
}

static bool alloc_lock(mutex_t* mutex, enum alloc_flags flags) {
    if (flags & ALLOC_NOSLEEP)
        return mutex_trylock(mutex);
    mutex_lock(mutex);
    return true;
}

static struct mem_slab* slab_create(struct mem_cache* cache, enum alloc_flags flags) {
    phys_t phys;
    if (mem_phys_alloc(cache->slab_pages, (flags & ALLOC_NOSLEEP) | ALLOC_NOZERO, &phys))
        return nullptr;

    void* base = HHDM_PTR(phys);
    struct mem_slab* slab;

    if (cache->is_inline) {
        slab = slab_trailer(base);
    } else {
        slab = mem_cache_alloc(mem_slab_cache, (flags & ALLOC_NOSLEEP) | ALLOC_NOZERO);
        if (!slab) {
            mem_phys_free(phys, cache->slab_pages);
            return nullptr;
        }
        for (size_t i = 0; i < cache->slab_pages; i++)
            page_from_addr(phys + i * page_size())->obj.owner = slab;
    }

    slab->cache = cache;
    slab->base = base;
    slab->free = nullptr;
    slab->refcount = 0;
    slab->chunks = cache->slab_chunks;
    slab->num_pages = cache->slab_pages;

    for (size_t i = slab->chunks; i-- > 0;) {
        void** chunk = base + i * cache->chunk_size;
        *chunk = slab->free;
        slab->free = chunk;
    }
    return slab;
}

static void slab_destroy(struct mem_cache* cache, struct mem_slab* slab) {
    const phys_t phys = (uintptr_t)slab->base - mem_hhdm_base;
    const size_t num_pages = slab->num_pages;
    if (!cache->is_inline) {
        for (size_t i = 0; i < num_pages; i++)
            page_from_addr(phys + i * page_size())->obj.owner = nullptr;
        mem_cache_free(mem_slab_cache, slab);
    }
    mem_phys_free(phys, num_pages);
}

// Allocates up to `max` objects at once in a cache.
// Returns the amount of objects allocated, and the start of the objects array in `*bufs`.
static size_t slab_alloc_batch(struct mem_cache* cache, void** bufs, size_t max, enum alloc_flags flags) {
    size_t count = 0;
    if (!alloc_lock(&cache->lock, flags))
        return 0;
    while (count < max) {
        struct mem_slab* slab = tailq_first(&cache->partial);
        if (!slab) {
            if (flags & ALLOC_NOSLEEP) {
                slab = slab_create(cache, flags);
            } else {
                mutex_unlock(&cache->lock);
                slab = slab_create(cache, flags);
                mutex_lock(&cache->lock);
            }
            if (!slab)
                break;
            cache->num_slabs++;
            tailq_insert_head(&cache->partial, slab, link);
        }

        while (count < max && slab->refcount < slab->chunks) {
            void** buf = slab->free;
            slab->free = *buf;
            slab->refcount++;
            bufs[count++] = buf;
        }
        if (slab->refcount == slab->chunks)
            tailq_remove(&cache->partial, slab, link);
    }
    mutex_unlock(&cache->lock);
    return count;
}

static void* slab_alloc(struct mem_cache* cache, enum alloc_flags flags) {
    void* buf;
    return slab_alloc_batch(cache, &buf, 1, flags) ? buf : nullptr;
}

static void slab_free(struct mem_cache* cache, void* buf) {
    struct mem_slab* slab = slab_of(cache, buf);

    mutex_lock(&cache->lock);
    const bool was_full = slab->refcount == slab->chunks;
    *(void**)buf = slab->free;
    slab->free = buf;
    slab->refcount--;

    if (slab->refcount == 0) {
        if (!was_full)
            tailq_remove(&cache->partial, slab, link);
        cache->num_slabs--;
        mutex_unlock(&cache->lock);
        slab_destroy(cache, slab);
        return;
    }

    if (was_full)
        tailq_insert_tail(&cache->partial, slab, link);
    mutex_unlock(&cache->lock);
}

static struct mem_magazine* depot_alloc(
    struct mem_cache* cache,
    struct mem_magazine** list,
    struct mem_magazine** spare_list,
    struct mem_magazine* spare,
    enum alloc_flags flags
) {
    if (!mutex_trylock(&cache->depot_lock)) {
        if (flags & ALLOC_NOSLEEP)
            return nullptr;
        mutex_lock(&cache->depot_lock);
        cache->depot_contention++;
    }

    struct mem_magazine* mag = *list;
    if (mag) {
        *list = mag->next;
        if (spare) {
            spare->next = *spare_list;
            *spare_list = spare;
        }
    }
    mutex_unlock(&cache->depot_lock);
    return mag;
}

static void depot_free(struct mem_cache* cache, struct mem_magazine** list, struct mem_magazine* mag) {
    mutex_lock(&cache->depot_lock);
    mag->next = *list;
    *list = mag;
    mutex_unlock(&cache->depot_lock);
}

static inline void mem_cpu_reload(struct mem_cpu_cache* ccp, struct mem_magazine* mag, int16_t rounds) {
    ccp->loaded_prev = ccp->loaded;
    ccp->rounds_prev = ccp->rounds;
    ccp->loaded = mag;
    ccp->rounds = rounds;
}

static inline struct mem_cpu_cache* mem_cpu_cache_enter(struct mem_cache* cache, enum alloc_flags flags) {
    struct mem_cpu_cache* ccp = &(*percpu_ref(mem_cpu_caches))[cache->id];
    return alloc_lock(&ccp->lock, flags) ? ccp : nullptr;
}

static inline void mem_cpu_cache_arm(struct mem_cache* cache, struct mem_cpu_cache* ccp) {
    if (ccp->num_rounds)
        return;
    struct mem_magtype* magtype = atomic_load(&cache->magtype, ATOMIC_ACQUIRE);
    if (magtype) {
        ccp->num_rounds = magtype->num_rounds;
        ccp->rounds = -1;
        ccp->rounds_prev = -1;
    }
}

void* mem_cache_alloc(struct mem_cache* cache, enum alloc_flags flags) {
    struct mem_cpu_cache* ccp = mem_cpu_cache_enter(cache, flags);
    void* buf = nullptr;

    while (ccp) {
        if (__likely(ccp->rounds > 0)) {
            buf = ccp->loaded->rounds[--ccp->rounds];
            break;
        }

        mem_cpu_cache_arm(cache, ccp);
        // If we have 0 rounds, this cache doesn't have magazines or we are resizing.
        if (ccp->num_rounds == 0)
            break;

        // If the last magazine still has objects remaining, use it.
        if (ccp->rounds_prev > 0) {
            mem_cpu_reload(ccp, ccp->loaded_prev, ccp->rounds_prev);
            continue;
        }

        // Both magazines are empty.
        struct mem_magazine* full =
            depot_alloc(cache, &cache->depot_full, &cache->depot_empty, ccp->loaded_prev, flags);
        if (!full) {
            if (ccp->rounds == 0) {
                ccp->rounds = slab_alloc_batch(cache, ccp->loaded->rounds, ccp->num_rounds, flags);
                if (ccp->rounds > 0)
                    continue;
            }
            break;
        }
        mem_cpu_reload(ccp, full, ccp->num_rounds);
    }
    if (ccp)
        mutex_unlock(&ccp->lock);

    // If we get here and don't have a buffer, no magazine was able to get an object.
    // In that case just allocate it directly.
    if (!buf) {
        buf = slab_alloc(cache, flags);
        if (!buf)
            return nullptr;
    }

    if (!(flags & ALLOC_NOZERO))
        memset(buf, 0, cache->size);
    return buf;
}

static bool mem_cpu_magazine_alloc(struct mem_cache* cache, struct mem_cpu_cache* ccp) {
    struct mem_magazine* empty = depot_alloc(cache, &cache->depot_empty, nullptr, nullptr, 0);
    if (empty) {
        if (ccp->loaded_prev)
            depot_free(cache, &cache->depot_full, ccp->loaded_prev);
        mem_cpu_reload(ccp, empty, 0);
        return true;
    }

    struct mem_magtype* magtype = atomic_load(&cache->magtype, ATOMIC_ACQUIRE);
    if (!magtype)
        return false;

    mutex_unlock(&ccp->lock);
    empty = slab_alloc(magtype->cache, ALLOC_NOSLEEP | ALLOC_NOZERO);
    mutex_lock(&ccp->lock);
    if (!empty)
        return false;

    if (ccp->num_rounds != magtype->num_rounds) {
        mutex_unlock(&ccp->lock);
        slab_free(magtype->cache, empty);
        mutex_lock(&ccp->lock);
        return true;
    }

    depot_free(cache, &cache->depot_empty, empty);
    return true;
}

void mem_cache_free(struct mem_cache* cache, void* buf) {
    struct mem_cpu_cache* ccp = mem_cpu_cache_enter(cache, 0);

    while (true) {
        mem_cpu_cache_arm(cache, ccp);
        if (ccp->num_rounds == 0)
            break;

        if (ccp->rounds >= 0 && ccp->rounds < ccp->num_rounds) {
            ccp->loaded->rounds[ccp->rounds++] = buf;
            mutex_unlock(&ccp->lock);
            return;
        }

        if (ccp->rounds_prev == 0) {
            mem_cpu_reload(ccp, ccp->loaded_prev, ccp->rounds_prev);
            continue;
        }

        if (!mem_cpu_magazine_alloc(cache, ccp))
            break;
    }
    mutex_unlock(&ccp->lock);

    slab_free(cache, buf);
}

static void magazine_destroy(
    struct mem_cache* cache,
    struct mem_magtype* magtype,
    struct mem_magazine* mag,
    int16_t rounds
) {
    for (int16_t i = 0; i < rounds; i++)
        slab_free(cache, mag->rounds[i]);
    slab_free(magtype->cache, mag);
}

static void mem_cache_magazine_purge(struct mem_cache* cache, struct mem_magtype* magtype) {
    const size_t count = percpu_count();
    for (size_t i = 0; i < count; i++) {
        struct mem_cpu_cache* ccp = &(*percpu_ref_on(percpu_cpu(i), mem_cpu_caches))[cache->id];

        mutex_lock(&ccp->lock);
        struct mem_magazine* loaded = ccp->loaded;
        struct mem_magazine* loaded_prev = ccp->loaded_prev;
        const int16_t rounds = ccp->rounds;
        const int16_t prounds = ccp->rounds_prev;
        ccp->loaded = nullptr;
        ccp->loaded_prev = nullptr;
        ccp->rounds = 0;
        ccp->rounds_prev = 0;
        ccp->num_rounds = 0;
        mutex_unlock(&ccp->lock);

        if (loaded)
            magazine_destroy(cache, magtype, loaded, rounds);
        if (loaded_prev)
            magazine_destroy(cache, magtype, loaded_prev, prounds);
    }

    struct mem_magazine* mag;
    while ((mag = depot_alloc(cache, &cache->depot_full, nullptr, nullptr, 0)))
        magazine_destroy(cache, magtype, mag, magtype->num_rounds);
    while ((mag = depot_alloc(cache, &cache->depot_empty, nullptr, nullptr, 0)))
        magazine_destroy(cache, magtype, mag, 0);
}

static void magazine_resize_locked(struct mem_cache* cache) {
    struct mem_magtype* magtype = atomic_load(&cache->magtype, ATOMIC_RELAXED);
    if (!magtype || cache->chunk_size >= magtype->max_obj_size)
        return;

    atomic_store(&cache->magtype, nullptr, ATOMIC_RELEASE);
    mem_cache_magazine_purge(cache, magtype);
    atomic_store(&cache->magtype, magtype + 1, ATOMIC_RELEASE);
}

void mem_cache_magazine_resize(struct mem_cache* cache) {
    mutex_lock(&mem_cache_lock);
    magazine_resize_locked(cache);
    mutex_unlock(&mem_cache_lock);
}

size_t mem_cache_num_slabs(struct mem_cache* cache) {
    mutex_lock(&cache->lock);
    const size_t count = cache->num_slabs;
    mutex_unlock(&cache->lock);
    return count;
}

size_t mem_cache_magsize(struct mem_cache* cache) {
    struct mem_magtype* magtype = atomic_load(&cache->magtype, ATOMIC_ACQUIRE);
    return magtype ? magtype->num_rounds : 0;
}

struct mem_cache* mem_cache_create(const char* name, size_t size, size_t align, enum mem_cache_flags flags) {
    const size_t pgsz = page_size();
    align = MAX(align, MEM_ALIGN); // Align to at least MEM_ALIGN.
    const size_t chunk_size = ROUND_UP(MAX(size, MEM_ALIGN), align);

    mutex_lock(&mem_cache_lock);
    struct mem_cache* cache = nullptr;
    for (size_t i = 0; i < MEM_MAX_CACHES; i++) {
        if (!mem_caches[i].active) {
            cache = &mem_caches[i];
            memset(cache, 0, sizeof(*cache));
            cache->id = i;
            cache->active = true;
            break;
        }
    }
    if (!cache) {
        mutex_unlock(&mem_cache_lock);
        return nullptr;
    }

    ksnprintf(cache->name, sizeof(cache->name), "%s", name);
    cache->size = size;
    cache->chunk_size = chunk_size;
    mutex_init(&cache->lock);
    mutex_init(&cache->depot_lock);
    tailq_init(&cache->partial);

    // If a chunk is smaller than 1/8th of a page, we can store the header inline.
    if (chunk_size < pgsz / 8) {
        cache->is_inline = true;
        cache->slab_pages = 1;
        cache->slab_chunks = (pgsz - sizeof(struct mem_slab)) / chunk_size;
    } else {
        size_t best_waste = SIZE_MAX;
        cache->is_inline = false;
        for (size_t chunks = 1; chunks <= 8; chunks++) {
            const size_t num_pages = DIV_CEIL(chunk_size * chunks, pgsz);
            const size_t fit = num_pages * pgsz / chunk_size;
            const size_t waste = (num_pages * pgsz) % chunk_size / fit;
            if (waste < best_waste) {
                best_waste = waste;
                cache->slab_pages = num_pages;
                cache->slab_chunks = fit;
            }
        }
    }

    if (__likely(!(flags & MEM_CACHE_NOMAGAZINE))) {
        struct mem_magtype* magtype = mem_magtypes;

        // Start with the first type whose min_obj_size is below the chunk size.
        // Larger objects start with fewer rounds so a CPU caches less memory
        // in them before the cache is ever promoted.
        while (magtype->min_obj_size >= chunk_size)
            magtype++;

        atomic_store(&cache->magtype, magtype, ATOMIC_RELEASE);
    }
    mutex_unlock(&mem_cache_lock);
    return cache;
}

void mem_cache_destroy(struct mem_cache* cache) {
    mutex_lock(&mem_cache_lock);

    struct mem_magtype* magtype = atomic_load(&cache->magtype, ATOMIC_RELAXED);
    if (magtype) {
        atomic_store(&cache->magtype, nullptr, ATOMIC_RELEASE);
        mem_cache_magazine_purge(cache, magtype);
    }

    assert(cache->num_slabs == 0, "Destroying cache \"%s\" with live buffers", cache->name);

    cache->active = false;
    mutex_unlock(&mem_cache_lock);
}

void* mem_alloc(size_t size, enum alloc_flags flags) {
    if (__unlikely(flags & (ALLOC_MEM20 | ALLOC_MEM32)))
        log_warn("ALLOC_MEM20 and ALLOC_MEM32 will be ignored! This is a bug!\n");

    if (__unlikely(size == 0))
        return nullptr;

    // If the allocation is bigger than the largest mem_alloc cache, allocate pages directly from the freelist.
    // This is going to be the case for allocations the size of a page or above.
    if (__unlikely(size > MEM_MAXBUF)) {
        phys_t phys;
        if (mem_phys_alloc(DIV_CEIL(size, page_size()), flags, &phys))
            return nullptr;
        return HHDM_PTR(phys);
    }

    void* buf = mem_cache_alloc(mem_alloc_table[(size - 1) >> MEM_ALIGN_SHIFT], flags | ALLOC_NOZERO);
    if (buf && !(flags & ALLOC_NOZERO))
        memset(buf, 0, size);

    return buf;
}

void mem_free(void* buf, size_t size) {
    if (__unlikely(!buf))
        return;

    // See above.
    if (__unlikely(size > MEM_MAXBUF)) {
        mem_phys_free((uintptr_t)buf - mem_hhdm_base, DIV_CEIL(size, page_size()));
        return;
    }

    assert(size != 0, "Freed %p with a size of 0", buf);
    mem_cache_free(mem_alloc_table[(size - 1) >> MEM_ALIGN_SHIFT], buf);
}

[[__init]]
void mem_cache_init(struct boot_info*) {
    const size_t pgsz = page_size();
    mutex_init(&mem_cache_lock);

    // Create a cache for slab headers.
    mem_slab_cache = mem_cache_create("mem_slab", sizeof(struct mem_slab), 0, MEM_CACHE_NOMAGAZINE);
    assert(mem_slab_cache, "Unable to create the slab cache");

    // Create caches for every magazine type.
    for (size_t i = 0; i < countof(mem_magtypes); i++) {
        struct mem_magtype* magtype = &mem_magtypes[i];

        char name[MEM_CACHE_NAME_MAX];
        ksnprintf(name, sizeof(name), "mem_magazine_%i", (int)magtype->num_rounds);
        const size_t size = sizeof(struct mem_magazine) + magtype->num_rounds * sizeof(void*);
        magtype->cache = mem_cache_create(name, size, MIN(size, CPU_CACHE_LINE_SIZE), MEM_CACHE_NOMAGAZINE);
        assert(magtype->cache, "Unable to create a magazine cache");
    }

    // Create mem_alloc caches for all sizes.
    size_t slot = 0;
    for (size_t i = 0; i < countof(MEM_ALLOC_SIZES); i++) {
        const size_t size = MEM_ALLOC_SIZES[i];

        // Find the best alignment for the size.
        size_t align = MEM_ALIGN;
        if (size % CPU_CACHE_LINE_SIZE == 0)
            align = CPU_CACHE_LINE_SIZE;
        if (size % pgsz == 0)
            align = pgsz;

        char name[MEM_CACHE_NAME_MAX];
        ksnprintf(name, sizeof(name), "mem_alloc_%zu", size);
        struct mem_cache* cache = mem_cache_create(name, size, align, 0);
        assert(cache, "Unable to create a mem_alloc cache");

        // Assign this cache for all sizes which are larger than the previous cache, but covered by this one.
        while (slot < (size >> MEM_ALIGN_SHIFT)) {
            mem_alloc_table[slot++] = cache;
        }
    }
}
INIT_TASK(mem_cache_stage, "mem.cache", mem_cache_init);
INIT_DEPENDS(mem_cache_stage, mem_init_stage);
INIT_DEPENDS(mem_cache_stage, mem_phys_stage);
INIT_ENTAILS(mem_cache_stage, mem_stage);

static struct timer update_timer;
static struct work update_work;

constexpr uint64_t UPDATE_INTERVAL_NS = SECS_TO_NS(15);
constexpr uint64_t TOLERATED_DEPOT_CONTENTION = 3;

// Grows a cache's magazines when its depot lock was contended too often in the last interval.
static void mem_update_fn(struct work* work) {
    mutex_lock(&mem_cache_lock);
    for (size_t i = 0; i < MEM_MAX_CACHES; i++) {
        struct mem_cache* cache = &mem_caches[i];
        if (!cache->active)
            continue;

        mutex_lock(&cache->depot_lock);
        const uint64_t contention = cache->depot_contention;
        const uint64_t delta = contention - cache->depot_contention_prev;
        cache->depot_contention_prev = contention;
        mutex_unlock(&cache->depot_lock);

        if (delta > TOLERATED_DEPOT_CONTENTION)
            magazine_resize_locked(cache);
    }
    mutex_unlock(&mem_cache_lock);

    timer_arm_after(&update_timer, UPDATE_INTERVAL_NS);
}

static void mem_update_tick(struct timer* timer) {
    work_queue(workqueue_system, &update_work);
}

[[__init]]
static void mem_cache_timers(struct boot_info*) {
    work_init(&update_work, mem_update_fn);
    timer_init(&update_timer, mem_update_tick);
    timer_arm_after(&update_timer, UPDATE_INTERVAL_NS);
}
INIT_TASK(mem_cache_timers_stage, "mem.cache_timers", mem_cache_timers);
INIT_DEPENDS(mem_cache_timers_stage, mem_cache_stage);
INIT_DEPENDS(mem_cache_timers_stage, workqueue_stage);

// TODO: Replace with actual virtual memory allocator.
static _Atomic(size_t) mapping_offset = 0;

uintptr_t vm_alloc(size_t length) {
    uintptr_t base = mem_mapping_addr();
    size_t offset = atomic_fetch_add(&mapping_offset, ROUND_UP(length, page_size()), ATOMIC_ACQUIRE);

    return base + offset;
}

void vm_free(uintptr_t ptr, size_t length) {
    // TODO
}
