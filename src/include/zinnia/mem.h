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
    ZN_VM_MAP_FIXED = 1 << 4,
};

// Creates a new virtual memory object.
zn_status_t zn_vmo_create(size_t length, zn_handle_t* out);

// Creates a new virtual memory object which points to a contiguous phyiscal memory region.
zn_status_t zn_vmo_create_phys(uintptr_t phys_addr, size_t length, zn_handle_t* out);

// Maps `bytes` of `vmo` at `vmo_offset` to `addr` in `vas`.
zn_status_t zn_vmo_map(
    zn_handle_t vmo,
    zn_handle_t vas,
    uintptr_t vmo_offset,
    uintptr_t* addr,
    size_t bytes,
    enum zn_vm_flags flags
);

// Creates a new virtual address space.
zn_status_t zn_vas_create(zn_handle_t* out);

#ifdef __cplusplus
}
#endif

#endif
