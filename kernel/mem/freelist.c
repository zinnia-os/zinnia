// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/compiler.h>
#include <lib/initgraph.h>
#include <lib/panic.h>
#include <lib/print.h>
#include <lib/spin.h>
#include <mem/alloc.h>
#include <mem/layout.h>
#include <mem/page.h>
#include <stdint.h>

static errno_t (*alloc_fn)(size_t num_pages, enum alloc_flags flags, phys_t* out);
static errno_t (*free_fn)(phys_t start, size_t num_pages);

errno_t mem_phys_alloc(size_t num_pages, enum alloc_flags flags, phys_t* out) {
    return alloc_fn(num_pages, flags, out);
}

errno_t mem_phys_free(phys_t start, size_t num_pages) {
    return free_fn(start, num_pages);
}

static struct page* pmm_head = nullptr;
static struct spinlock pmm_lock = {0};
static size_t pmm_total_free = 0;

static errno_t freelist_alloc(size_t num_pages, enum alloc_flags flags, phys_t* out) {
    const size_t pgsz = page_size();
    const size_t bytes = num_pages * pgsz;

    spin_lock(&pmm_lock);

    phys_t limit = UINTPTR_MAX;
    if (flags & ALLOC_MEM20) {
        limit = (phys_t)1 << 20;
    } else if (flags & ALLOC_MEM32) {
        limit = (phys_t)1 << 32;
    }

    struct page* it = pmm_head;
    struct page* prev_it = nullptr;

    for (; it; prev_it = it, it = it->freelist.next) {
        // If the region doesn't have enough pages left or is too high up, continue searching.
        if (__unlikely(it->freelist.count < num_pages))
            continue;

        const phys_t page_addr = (((uintptr_t)it - (uintptr_t)vm_pfndb) / sizeof(struct page)) * pgsz;
        if (page_addr + bytes > limit)
            continue;

        // If the current block has exactly enough pages left, consume it entirely.
        if (__unlikely(it->freelist.count == num_pages)) {
            if (prev_it)
                prev_it->freelist.next = it->freelist.next;
            else
                pmm_head = it->freelist.next;

            it->freelist.next = nullptr;
            it->freelist.count = 0;

            if (__likely(out))
                *out = page_addr;
        }
        // If not, we can just shrink the block.
        else if (page_addr + it->freelist.count * pgsz <= limit) {
            it->freelist.count -= num_pages;

            if (__likely(out))
                *out = page_addr + (it->freelist.count * pgsz);
        } else {
            struct page* rest = it + num_pages;
            rest->freelist.count = it->freelist.count - num_pages;
            rest->freelist.next = it->freelist.next;

            if (prev_it)
                prev_it->freelist.next = rest;
            else
                pmm_head = rest;

            it->freelist.next = nullptr;
            it->freelist.count = 0;

            if (__likely(out))
                *out = page_addr;
        }
        goto success;
    }

    spin_unlock(&pmm_lock);
    return ENOMEM;

success:
    pmm_total_free -= num_pages;
    spin_unlock(&pmm_lock);

    // If the NOZERO flag is *not* specified, zero memory.
    if (__likely(out) && __likely(!(flags & ALLOC_NOZERO))) {
        memset(HHDM_PTR(*out), 0, bytes);
    }
    return 0;
}

static void freelist_insert(struct page* page, size_t num_pages) {
    struct page* prev = nullptr;
    struct page* next = pmm_head;
    while (next && next < page) {
        prev = next;
        next = next->freelist.next;
    }

    page->freelist.count = num_pages;
    page->freelist.next = next;

    if (next && page + page->freelist.count == next) {
        page->freelist.count += next->freelist.count;
        page->freelist.next = next->freelist.next;
        next->freelist.next = nullptr;
        next->freelist.count = 0;
    }

    if (prev && prev + prev->freelist.count == page) {
        prev->freelist.count += page->freelist.count;
        prev->freelist.next = page->freelist.next;
        page->freelist.next = nullptr;
        page->freelist.count = 0;
    } else if (prev) {
        prev->freelist.next = page;
    } else {
        pmm_head = page;
    }
}

static errno_t freelist_free(phys_t addr, size_t num_pages) {
    spin_lock(&pmm_lock);

    freelist_insert(&vm_pfndb[addr / page_size()], num_pages);
    pmm_total_free += num_pages;

    spin_unlock(&pmm_lock);
    return 0;
}

static struct phys_mem bump_mem = {0};
static struct phys_mem* bump_region = nullptr;

static errno_t bump_alloc(size_t num_pages, enum alloc_flags flags, phys_t* out) {
    const size_t bytes = num_pages * page_size();
    if (bytes > bump_mem.length)
        return ENOMEM;

    *out = bump_mem.address;
    bump_mem.length -= bytes;
    bump_mem.address += bytes;

    if (!(flags & ALLOC_NOZERO)) {
        memset(HHDM_PTR(*out), 0, bytes);
    }

    return 0;
}

static errno_t bump_free(phys_t addr, size_t num_pages) {
    // We don't free bootstrap memory.
    panic("Attempted to free bootstrap memory!");
}

void mem_phys_init(struct boot_info* info) {
    struct phys_mem* map = info->mem_map;
    size_t length = info->num_mem_maps;

    alloc_fn = freelist_alloc;
    free_fn = freelist_free;
    bump_region->address = bump_mem.address;
    bump_region->length = bump_mem.length;

    for (size_t i = 0; i < length; i++) {
        // Regions smaller than a page are useless.
        if (map[i].length < page_size() || map[i].usage != PHYS_USABLE)
            continue;

        const size_t num_pages = map[i].length / page_size();
        freelist_insert(&vm_pfndb[map[i].address / page_size()], num_pages);
        pmm_total_free += num_pages;
    }

    const size_t free_bytes = (pmm_total_free * page_size());
    log("freelist: Total available memory: %zu MiB\n", free_bytes / 1024 / 1024);
}
INIT_TASK(mem_phys_stage, "mem.phys_init", mem_phys_init);
INIT_DEPENDS(mem_phys_stage, mem_init_stage);
INIT_ENTAILS(mem_phys_stage, mem_stage);

static void mem_phys_bootstrap(struct boot_info* info) {
    struct phys_mem* map = info->mem_map;
    size_t map_len = info->num_mem_maps;
    phys_t kernel_phys = info->phys_base;
    uintptr_t tmp_hhdm = info->hhdm_base;

    // Set up the bootstrap allocator.
    struct phys_mem* largest = nullptr;
    for (size_t i = 0; i < map_len; i++) {
        if (map[i].usage == PHYS_USABLE && (!largest || map[i].length > largest->length))
            largest = &map[i];
    }
    assert(largest && largest->length >= page_size(), "No usable memory for bootstrap allocator");

    log("Using region [%p - %p] for bootstrap allocator\n",
        (void*)largest->address,
        (void*)(largest->address + largest->length - 1));

    alloc_fn = bump_alloc;
    free_fn = bump_free;

    // Save the region by reference and value.
    // We can't directly modify the region by reference, because that
    // interferes with e.g. HHDM mapping logic.
    bump_mem = *largest;
    bump_region = largest;
}
INIT_TASK(mem_phys_bootstrap_stage, "mem.phys_bootstrap", mem_phys_bootstrap);
INIT_ENTAILS(mem_phys_bootstrap_stage, mem_init_stage);
