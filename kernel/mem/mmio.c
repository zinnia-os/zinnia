#include <kernel/mmio.h>
#include <kernel/pmap.h>
#include <kernel/virt.h>
#include <kernel/vmspace.h>
#include <bits/mem.h>

void* mmio_new(phys_t addr, size_t length) {
    void* ptr = vm_alloc(length);
    for (size_t i = 0; i < length; i += arch_mem_page_size()) {

        if (pmap_map(&kernel_space.pmap, (uintptr_t)ptr + i, addr + i, PTE_READ | PTE_WRITE, CACHE_MMIO)) {
            vm_free(ptr, length);
            return nullptr;
        }
    }

    return ptr;
}

void mmio_free(void* ptr, size_t length) {
    // TODO
}
