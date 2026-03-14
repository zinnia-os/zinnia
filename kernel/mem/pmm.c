#include <kernel/alloc.h>
#include <kernel/compiler.h>
#include <kernel/panic.h>
#include <kernel/print.h>
#include <kernel/spin.h>
#include <kernel/virt.h>
#include <stdint.h>
#include <string.h>

uintptr_t mem_hhdm_base = 0;

static errno_t (*alloc_fn)(size_t num_pages, enum alloc_flags flags, phys_t* out);
static errno_t (*free_fn)(phys_t start, size_t num_pages);

errno_t mem_phys_alloc(size_t num_pages, enum alloc_flags flags, phys_t* out) {
    return alloc_fn(num_pages, flags, out);
}

errno_t mem_phys_free(phys_t start, size_t num_pages) {
    return free_fn(start, num_pages);
}

static struct phys_mem bump_mem = {0};
static struct phys_mem* bump_region = nullptr;

static errno_t bump_alloc(size_t num_pages, enum alloc_flags flags, phys_t* out) {
    const size_t bytes = num_pages * arch_mem_page_size();
    if (((intptr_t)bump_mem.length - bytes) <= 0)
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

void mem_phys_bootstrap(struct phys_mem* mem) {
    alloc_fn = bump_alloc;
    free_fn = bump_free;

    // Save the region by reference and value.
    // We can't directly modify the region by reference, because that
    // interferes with e.g. HHDM mapping logic.
    bump_mem = *mem;
    bump_region = mem;
}

static struct page* pmm_head = nullptr;
static struct spinlock pmm_lock = {0};
static size_t pmm_total_free = 0;

static errno_t freelist_alloc(size_t num_pages, enum alloc_flags flags, phys_t* out) {
    spin_lock(&pmm_lock);

    const size_t bytes = num_pages * arch_mem_page_size();
    phys_t limit = UINTPTR_MAX;
    if (flags & ALLOC_MEM20) {
        limit = (phys_t)1 << 20;
    } else if (flags & ALLOC_MEM32) {
        limit = (phys_t)1 << 32;
    }

    struct page* it = pmm_head;
    struct page* prev_it = nullptr;

    while (it) {
        const phys_t page_addr = (((uintptr_t)it - (uintptr_t)vm_pfndb) / sizeof(struct page)) * arch_mem_page_size();
        // If the region is too high up or doesn't have enough pages left, continue searching.
        if (page_addr + bytes >= limit || __unlikely(it->freelist.count < num_pages)) {
            prev_it = it;
            it = it->freelist.next;
            continue;
        }

        // If the current block has exactly enough pages left, consume it entirely.
        if (__unlikely(it->freelist.count == num_pages)) {
            if (prev_it) {
                prev_it->freelist.next = it->freelist.next;
            } else {
                pmm_head = it->freelist.next;
            }

            it->freelist.next = nullptr;
            it->freelist.count = 0;

            if (__likely(out))
                *out = page_addr;
        }
        // If not, we can just shrink the block.
        else {
            it->freelist.count -= num_pages;

            if (__likely(out))
                *out = page_addr + (it->freelist.count * arch_mem_page_size());
        }
        goto success;
    }

    spin_unlock(&pmm_lock);
    return ENOMEM;

success:
    // If the NOZERO flag is *not* specified, zero memory.
    if (__likely(out) && __likely(!(flags & ALLOC_NOZERO))) {
        memset(HHDM_PTR(*out), 0, bytes);
    }
    pmm_total_free -= num_pages;
    spin_unlock(&pmm_lock);
    return 0;
}

static errno_t freelist_free(phys_t addr, size_t num_pages) {
    spin_lock(&pmm_lock);

    struct page* page = &vm_pfndb[addr / arch_mem_page_size()];
    page->freelist.count = num_pages;
    page->freelist.next = pmm_head;
    pmm_head = page;

    spin_unlock(&pmm_lock);
    return 0;
}

void mem_phys_init(struct phys_mem* map, size_t length) {
    alloc_fn = freelist_alloc;
    free_fn = freelist_free;
    bump_region->address = bump_mem.address;
    bump_region->length = bump_mem.length;

    for (size_t i = 0; i < length; i++) {
        // Regions smaller than a page are useless.
        if (map[i].length < arch_mem_page_size() || map[i].usage != PHYS_USABLE)
            continue;

        struct page* page = &vm_pfndb[map[i].address / arch_mem_page_size()];
        page->freelist.count = map[i].length / arch_mem_page_size();
        page->freelist.next = pmm_head;
        pmm_head = page;
        pmm_total_free += page->freelist.count;
    }

    const size_t free_bytes = (pmm_total_free * arch_mem_page_size());
    kprintf("mem: Total available memory: %zu MiB\n", free_bytes / 1024 / 1024);
}
