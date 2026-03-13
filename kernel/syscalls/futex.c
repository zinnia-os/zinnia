#include <kernel/futex.h>
#include <kernel/syscall.h>

SYSCALL_DEFINE(futex_wait, ctx) {
    __user int* addr = (__user int*)ctx->ARCH_CTX_A0;
    int expected = ctx->ARCH_CTX_A1;

    return (sc_result_t){
        .err = futex_wait(addr, expected),
    };
}

SYSCALL_DEFINE(futex_wake, ctx) {
    __user int* addr = (__user int*)ctx->ARCH_CTX_A0;
    int count = ctx->ARCH_CTX_A1;

    return (sc_result_t){
        .err = futex_wake(addr, count),
    };
}
