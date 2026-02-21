#include <zinnia/channel.h>
#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <kernel/syscalls.h>

zn_status_t syscall_channel_create(struct arch_context* ctx) {
    enum zn_channel_flags flags = ctx->ARCH_CTX_A0;
    __user zn_handle_t* endpoint0 = (__user zn_handle_t*)ctx->ARCH_CTX_A1;
    __user zn_handle_t* endpoint1 = (__user zn_handle_t*)ctx->ARCH_CTX_A2;

    return ZN_ERR_UNSUPPORTED;
}

zn_status_t syscall_channel_wait(struct arch_context* ctx) {
    return ZN_ERR_UNSUPPORTED;
}

zn_status_t syscall_channel_read(struct arch_context* ctx) {
    return ZN_ERR_UNSUPPORTED;
}

zn_status_t syscall_channel_write(struct arch_context* ctx) {
    return ZN_ERR_UNSUPPORTED;
}
