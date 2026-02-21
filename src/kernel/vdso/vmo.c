#include <zinnia/handle.h>
#include <zinnia/mem.h>
#include <zinnia/status.h>
#include <vdso/common.h>
#include <vdso/syscall_stubs.h>
#include <stddef.h>

VDSO_FUNC(zn_status_t, zn_vmo_create, size_t length, zn_handle_t* out) {
    return zn_syscall2(length, out, ZN_SYSCALL_VMO_CREATE);
}

VDSO_FUNC(zn_status_t, zn_vmo_create_phys, uintptr_t phys_addr, size_t length, zn_handle_t* out) {
    return zn_syscall3(phys_addr, length, out, ZN_SYSCALL_VMO_CREATE_PHYS);
}

VDSO_FUNC(
    zn_status_t,
    zn_vmo_map,
    zn_handle_t vmo,
    zn_handle_t vas,
    uintptr_t vmo_offset,
    uintptr_t* addr,
    size_t bytes,
    enum zn_vm_flags flags
) {
    return zn_syscall6(vmo, vas, vmo_offset, addr, bytes, flags, ZN_SYSCALL_VMO_MAP);
}
