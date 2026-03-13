#include <kernel/syscall.h>

// Declare all syscall functions.
#define SYSCALL(num, name) sc_result_t syscall_##name(struct arch_context*);
#include <kernel/syscall_list.h>
#undef SYSCALL

// Put all functions into the syscall table.
static const syscall_fn_t syscall_table[] =  {
#define SYSCALL(num, name) [num] = syscall_##name,
#include <kernel/syscall_list.h>
#undef SYSCALL
};

void syscall_dispatch(struct arch_context* ctx) {
    const size_t num = ctx->ARCH_CTX_NUM;
    if (__unlikely(num >= ARRAY_SIZE(syscall_table)) || __unlikely(syscall_table[num] == nullptr)) {
        ctx->ARCH_CTX_RET0 = 0;
        ctx->ARCH_CTX_RET1 = ENOSYS;
        return;
    }

    sc_result_t res = syscall_table[num](ctx);
    ctx->ARCH_CTX_RET0 = res.val;
    ctx->ARCH_CTX_RET1 = res.err;
}
