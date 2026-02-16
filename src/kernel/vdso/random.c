#include <zinnia/random.h>
#include <zinnia/status.h>
#include <vdso/common.h>
#include <vdso/syscall_stubs.h>
#include <stddef.h>

VDSO_FUNC(zn_status_t, zn_random_get, void* addr, size_t len) {
    return zn_syscall2(addr, len, ZN_SYSCALL_RANDOM_GET);
}

VDSO_FUNC(zn_status_t, zn_random_entropy, const void* addr, size_t len) {
    return zn_syscall2(addr, len, ZN_SYSCALL_RANDOM_ENTROPY);
}
