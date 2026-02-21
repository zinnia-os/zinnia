#include <zinnia/handle.h>
#include <vdso/common.h>
#include <vdso/syscall_stubs.h>

VDSO_FUNC(zn_status_t, zn_vas_create, zn_handle_t* out) {
    return zn_syscall1(out, ZN_SYSCALL_VAS_CREATE);
}
