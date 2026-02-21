#include <zinnia/system.h>
#include <vdso/common.h>
#include <vdso/syscall_stubs.h>

VDSO_FUNC(void, zn_log, const char* message, size_t len) {
    zn_syscall2(message, len, ZN_SYSCALL_LOG);
}

VDSO_FUNC(size_t, zn_page_size) {
    size_t size = 0;
    zn_syscall1(&size, ZN_SYSCALL_PAGE_SIZE);
    return size;
}

VDSO_FUNC(zn_status_t, zn_powerctl) {
    return ZN_ERR_UNSUPPORTED;
}
