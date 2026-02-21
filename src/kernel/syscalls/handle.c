#include <zinnia/handle.h>
#include <zinnia/rights.h>
#include <zinnia/status.h>
#include <kernel/percpu.h>
#include <kernel/syscalls.h>

zn_status_t syscall_handle_clone(struct arch_context* ctx) {
    zn_handle_t handle = ctx->ARCH_CTX_A0;
    zn_rights_t cloned_rights = ctx->ARCH_CTX_A1;
    __user zn_handle_t* cloned = (__user zn_handle_t*)ctx->ARCH_CTX_A2;

    struct task* current = percpu_get()->sched.current;

    return ZN_ERR_UNSUPPORTED;
}

zn_status_t syscall_handle_drop(struct arch_context* ctx) {
    zn_handle_t handle = ctx->ARCH_CTX_A0;
    struct task* current = percpu_get()->sched.current;

    // TODO

    return ZN_OK;
}

zn_status_t syscall_handle_validate(struct arch_context* ctx) {
    zn_handle_t handle = ctx->ARCH_CTX_A0;
    struct task* current = percpu_get()->sched.current;
    struct namespace* ns = current->namespace;

    return ZN_ERR_UNSUPPORTED;
}
