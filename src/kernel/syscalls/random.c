#include <zinnia/status.h>
#include <kernel/syscalls.h>

zn_status_t syscall_random_entropy(struct arch_context* ctx) {
    return ZN_ERR_UNSUPPORTED;
}

zn_status_t syscall_random_get(struct arch_context* ctx) {
    return ZN_ERR_UNSUPPORTED;
}
