#include <common/utils.h>
#include <kernel/alloc.h>
#include <kernel/virt.h>
#include <stdatomic.h>
#include <stddef.h>

struct page* vm_pfndb = nullptr;

// TODO: Replace with actual allocator.

static size_t mapping_offset = 0;

void* vm_alloc(size_t length) {
    uintptr_t base = arch_mem_mapping_addr();
    size_t offset = atomic_fetch_add(&mapping_offset, ALIGN_UP(length, arch_mem_page_size()));

    return (void*)(base + offset);
}

void vm_free(void* ptr, size_t length) {
    // TODO
}

uintptr_t vm_page_addr(struct page* page) {
    return (((uintptr_t)page - (uintptr_t)vm_pfndb) / sizeof(struct page)) * arch_mem_page_size();
}
