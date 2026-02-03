#ifndef MENIX_MEM_H
#define MENIX_MEM_H

#include <menix/handle.h>
#include <menix/status.h>
#include <stddef.h>

// Virtual memory flags.
enum menix_vm_flags {
    MENIX_VM_READ = 1 << 0,
    MENIX_VM_WRITE = 1 << 1,
    MENIX_VM_EXEC = 1 << 2,
    MENIX_VM_SHARED = 1 << 3,
};

enum menix_cache_type {
    // Generic memory
    MENIX_CACHE_NORMAL,
    // Write combining
    MENIX_CACHE_WC,
    // Memory-mapped IO
    MENIX_CACHE_MMIO,
};

// Allocates memory
menix_status_t menix_mem_alloc(size_t length, menix_handle_t* out);

menix_status_t menix_mem_map(menix_handle_t mem, void* addr, size_t len, enum menix_vm_flags flags);

menix_status_t menix_mem_unmap(menix_handle_t mem, void* addr, size_t len, enum menix_vm_flags flags);

#endif
