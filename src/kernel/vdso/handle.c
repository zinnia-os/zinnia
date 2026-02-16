#include <zinnia/handle.h>
#include <zinnia/rights.h>
#include <zinnia/status.h>
#include <vdso/common.h>
#include <vdso/syscall_stubs.h>

VDSO_FUNC(zn_status_t, zn_handle_validate, zn_handle_t handle) {
    return zn_syscall1(handle, ZN_SYSCALL_HANDLE_VALIDATE);
}

VDSO_FUNC(zn_status_t, zn_handle_drop, zn_handle_t handle) {
    return zn_syscall1(handle, ZN_SYSCALL_HANDLE_DROP);
}

VDSO_FUNC(zn_status_t, zn_handle_clone, zn_handle_t handle, zn_rights_t cloned_rights, zn_handle_t* cloned) {
    return zn_syscall3(handle, cloned_rights, cloned, ZN_SYSCALL_HANDLE_CLONE);
}
