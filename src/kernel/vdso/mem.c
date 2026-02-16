#include <zinnia/handle.h>
#include <zinnia/mem.h>
#include <zinnia/status.h>
#include <vdso/common.h>
#include <vdso/syscall_stubs.h>
#include <stddef.h>

VDSO_FUNC(zn_status_t, zn_vmo_create, size_t length, zn_handle_t* out) {
    return ZN_ERR_UNSUPPORTED;
}

VDSO_FUNC(zn_status_t, zn_vmo_create_phys, uintptr_t phys_addr, size_t length, zn_handle_t* out) {
    return ZN_ERR_UNSUPPORTED;
}

VDSO_FUNC(zn_status_t, zn_vas_create, zn_handle_t* out) {
    return zn_syscall1((zn_arg_t)out, ZN_SYSCALL_VAS_CREATE);
}

VDSO_FUNC(zn_status_t, zn_vas_map, zn_handle_t space, void* addr, size_t len, enum zn_vm_flags flags) {
    return ZN_ERR_UNSUPPORTED;
}

VDSO_FUNC(zn_status_t, zn_vas_protect, zn_handle_t space, void* addr, size_t len, enum zn_vm_flags flags) {
    return ZN_ERR_UNSUPPORTED;
}

VDSO_FUNC(zn_status_t, zn_vas_unmap, zn_handle_t space, void* addr, size_t len) {
    return ZN_ERR_UNSUPPORTED;
}
