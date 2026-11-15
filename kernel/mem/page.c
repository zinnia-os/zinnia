// SPDX-License-Identifier: BSD-3-Clause

#include <arch/mem.h>
#include <lib/atomic.h>
#include <lib/park.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <stddef.h>

uintptr_t mem_hhdm_base = 0;
struct page* vm_pfndb = nullptr;
struct page* vm_pfndb_end = nullptr;

errno_t page_alloc(enum alloc_flags flags, struct page** out) {
    phys_t addr;
    errno_t status = mem_phys_alloc(1, flags, &addr);
    if (status)
        return status;

    struct page* page = page_from_addr(addr);
    page->type = PAGE_PHYS;
    page->obj.owner = nullptr;
    page->obj.index = 0;
    atomic_store(&page->flags, 0, ATOMIC_RELAXED);
    atomic_store(&page->holds, 1, ATOMIC_RELEASE);
    *out = page;
    return 0;
}

void page_hold(struct page* page) {
    atomic_fetch_add(&page->holds, 1, ATOMIC_RELAXED);
}

bool page_try_hold(struct page* page) {
    size_t holds = atomic_load(&page->holds, ATOMIC_RELAXED);
    while (holds) {
        if (atomic_compare_exchange_weak(&page->holds, &holds, holds + 1, ATOMIC_ACQUIRE, ATOMIC_RELAXED))
            return true;
    }
    return false;
}

void page_unhold(struct page* page) {
    if (atomic_fetch_sub(&page->holds, 1, ATOMIC_RELEASE) != 1)
        return;
    atomic_fence(ATOMIC_ACQUIRE);
    atomic_store(&page->flags, 0, ATOMIC_RELAXED);
    page->obj.owner = nullptr;
    mem_phys_free(page_addr(page), 1);
}

bool page_try_busy(struct page* page) {
    return !(atomic_fetch_or(&page->flags, PAGE_BUSY, ATOMIC_ACQUIRE) & PAGE_BUSY);
}

void page_unbusy(struct page* page) {
    const uint32_t old = atomic_fetch_and(&page->flags, ~(PAGE_BUSY | PAGE_WANTED), ATOMIC_RELEASE);
    if (old & PAGE_WANTED)
        unpark_all(&page->flags, 0);
}

static bool still_busy(void* ctx) {
    struct page* page = ctx;
    uint32_t flags = atomic_load(&page->flags, ATOMIC_RELAXED);
    while (flags & PAGE_BUSY) {
        if (atomic_compare_exchange_weak(&page->flags, &flags, flags | PAGE_WANTED, ATOMIC_RELAXED, ATOMIC_RELAXED))
            return true;
    }
    return false;
}

void page_wait(struct page* page) {
    park(&page->flags, still_busy, page, 0);
}

uintptr_t page_addr(struct page* page) {
    return (((uintptr_t)page - (uintptr_t)vm_pfndb) / sizeof(struct page)) * arch_mem_page_size();
}

struct page* page_from_addr(phys_t addr) {
    return &vm_pfndb[addr / arch_mem_page_size()];
}
