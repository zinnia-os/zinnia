// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>
#include <lib/errno.h>
#include <lib/hashmap.h>
#include <lib/types.h>
#include <mem/alloc.h>
#include <bits/mem.h>
#include <stddef.h>
#include <stdint.h>

static inline size_t page_size() {
    return arch_page_size();
}

enum page_type : uint8_t {
    PAGE_PHYS = 0, // Regular physical memory.
};

enum page_flags : uint32_t {
    PAGE_BUSY = 1 << 0,
    PAGE_WANTED = 1 << 1,
    PAGE_DIRTY = 1 << 2,
    PAGE_ANON = 1 << 3,
};

struct page {
    enum page_type type;
    _Atomic(uint32_t) flags;
    _Atomic(size_t) holds;
    union {
        struct {
            struct page* next; // Pointer to the next chunk.
            size_t count;      // Amount of free pages.
        } freelist;
        struct {
            void* owner;
            uint64_t index;
        } obj;
    };
};
static_assert(0x1000 % sizeof(struct page) == 0, "struct must fit cleanly inside a page!");
static_assert(sizeof(struct page) <= 64, "struct must be smaller than 64 bytes!");

// Base address of the HHDM.
extern uintptr_t mem_hhdm_base;

static inline void* HHDM_PTR(phys_t paddr) {
    return (void*)(mem_hhdm_base + paddr);
}

// Base address of the `struct page` array.
extern struct page* vm_pfndb;
extern struct page* vm_pfndb_end;

errno_t page_alloc(enum alloc_flags flags, struct page** out);
void page_hold(struct page* page);
bool page_try_hold(struct page* page);
void page_unhold(struct page* page);
bool page_try_busy(struct page* page);
void page_unbusy(struct page* page);
void page_wait(struct page* page);
uintptr_t page_addr(struct page* page);
struct page* page_from_addr(phys_t addr);

// Allocator for virtual memory.
struct vm_alloc {
    uintptr_t start_page;
    uintptr_t end_page;
    uintptr_t next_fit;
    hashmap(int, void);
};
