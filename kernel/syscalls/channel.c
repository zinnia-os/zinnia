#include <menix/channel.h>
#include <menix/status.h>
#include <kernel/compiler.h>

menix_status_t syscall_channel_create(
    enum menix_channel_flags flags,
    __user menix_handle_t* endpoint0,
    __user menix_handle_t* endpoint1
) {
    return MENIX_ERR_UNSUPPORTED;
}

menix_status_t syscall_channel_connect(
    menix_handle_t channel,
    size_t num_handles,
    size_t num_bytes,
    __user menix_handle_t** out_handle_buf,
    __user void** out_data_buf
) {
    return MENIX_ERR_UNSUPPORTED;
}
