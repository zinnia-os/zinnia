#ifndef ZINNIA_MEM_H
#define ZINNIA_MEM_H

#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Virtual memory flags.
enum zn_vm_flags {
    ZN_VM_MAP_READ = 1 << 0,
    ZN_VM_MAP_WRITE = 1 << 1,
    ZN_VM_MAP_EXEC = 1 << 2,
    ZN_VM_MAP_SHARED = 1 << 3,
};

// Creates a new virtual memory object.
zn_status_t zn_vmo_create(size_t length, zn_handle_t* out);

// Creates a new virtual memory object which points to a contiguous phyiscal memory region.
zn_status_t zn_vmo_create_phys(uintptr_t phys_addr, size_t length, zn_handle_t* out);

// Creates a new virtual address space.
zn_status_t zn_vas_create(zn_handle_t* out);

zn_status_t zn_vas_map(zn_handle_t space, void* addr, size_t len, enum zn_vm_flags flags);

zn_status_t zn_vas_protect(zn_handle_t space, void* addr, size_t len, enum zn_vm_flags flags);

zn_status_t zn_vas_unmap(zn_handle_t space, void* addr, size_t len);

#ifdef __cplusplus
}
#endif

#endif
