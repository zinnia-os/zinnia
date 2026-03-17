#include <kernel/archctl.h>
#include <kernel/syscall.h>

SYSCALL_DEFINE(archctl, ctx) {
    sc_result_t ret = {
        .err = EINVAL,
    };

    const enum archctl_op op = ctx->ARCH_CTX_A0;
    const uintptr_t value = ctx->ARCH_CTX_A1;

    ret.err = arch_archctl(op, value);

    return ret;
}
