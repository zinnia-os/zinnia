#include <zinnia/archctl.h>
#include <zinnia/status.h>
#include <vdso/common.h>
#include <vdso/syscall_stubs.h>

VDSO_FUNC(zn_status_t, zn_archctl, zn_archctl_t op, void* value) {
    return zn_syscall2(op, value, ZN_SYSCALL_ARCHCTL);
}
