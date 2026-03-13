#include <kernel/archctl.h>
#include <kernel/syscall.h>

SYSCALL_DEFINE(archctl, ctx) {
    enum archctl_op op = ctx->ARCH_CTX_A0;
    uintptr_t value = ctx->ARCH_CTX_A1;
    return (sc_result_t){.err = arch_archctl(op, value)};
}
