#pragma once

#include <stddef.h>
#include <stdint.h>

enum page_type : uint32_t {
    PAGE_PHYS = 0, // Regular physical memory.
};

struct page {
    enum page_type type;
    uint32_t flags;
    size_t refcount;
    union {
        struct {
            struct page* next; // Pointer to the next chunk.
            size_t count;      // Amount of free pages.
        } freelist;
    };
};
static_assert(0x1000 % sizeof(struct page) == 0, "struct must fit cleanly inside a page!");
static_assert(sizeof(struct page) <= 64, "struct must be smaller than 64 bytes!");

void* vm_alloc(size_t length);
void vm_free(void* addr, size_t length);

// Base address of the `struct page` array.
extern struct page* vm_pfndb;

uintptr_t vm_page_addr(struct page* page);
