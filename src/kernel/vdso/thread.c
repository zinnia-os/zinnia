#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <zinnia/thread.h>
#include <vdso/common.h>
#include <vdso/syscall_stubs.h>
#include <string.h>

VDSO_FUNC(
    zn_status_t,
    zn_thread_create,
    zn_handle_t universe,
    const char* name,
    size_t name_len,
    enum zn_thread_flags flags
) {
    return zn_syscall4(universe, name, name_len, flags, ZN_SYSCALL_THREAD_CREATE);
}

VDSO_FUNC(zn_status_t, zn_thread_start, zn_handle_t thread, uintptr_t ip, uintptr_t sp, uintptr_t arg) {
    return zn_syscall4(thread, ip, sp, arg, ZN_SYSCALL_THREAD_START);
}

VDSO_FUNC(void, zn_thread_exit) {
    zn_syscall0(ZN_SYSCALL_THREAD_EXIT);
}
