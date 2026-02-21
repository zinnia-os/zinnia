#include <zinnia/archctl.h>
#include <kernel/syscalls.h>

zn_status_t arch_archctl(zn_archctl_t op, uintptr_t value);

zn_status_t syscall_archctl(struct arch_context* ctx) {
    return arch_archctl((zn_archctl_t)ctx->ARCH_CTX_A0, (uintptr_t)ctx->ARCH_CTX_A1);
}
