#ifndef ZINNIA_MEM_H
#define ZINNIA_MEM_H

#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <zinnia/syscall_stubs.h>
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
static inline zn_status_t zn_vmo_create(size_t length, zn_handle_t* out) {
    return zn_syscall2(length, out, ZN_SYSCALL_VMO_CREATE);
}

// Creates a new virtual memory object which points to a contiguous phyiscal memory region.
static inline zn_status_t zn_vmo_create_phys(uintptr_t phys_addr, size_t length, zn_handle_t* out) {
    return zn_syscall3(phys_addr, length, out, ZN_SYSCALL_VMO_CREATE_PHYS);
}

// Maps `bytes` of `vmo` at `vmo_offset` to `addr` in `vas`.
static inline zn_status_t zn_vmo_map(
    zn_handle_t vmo,
    zn_handle_t vas,
    uintptr_t vmo_offset,
    uintptr_t* addr,
    size_t bytes,
    enum zn_vm_flags flags
) {
    return zn_syscall6(vmo, vas, vmo_offset, addr, bytes, flags, ZN_SYSCALL_VMO_MAP);
}

// Creates a new virtual address space.
static inline zn_status_t zn_vas_create(zn_handle_t* out) {
    return zn_syscall1(out, ZN_SYSCALL_VAS_CREATE);
}

#ifdef __cplusplus
}
#endif

#endif
