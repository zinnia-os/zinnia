#include <zinnia/channel.h>
#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <common/compiler.h>

zn_status_t syscall_channel_create(
    enum zn_channel_flags flags,
    __user zn_handle_t* endpoint0,
    __user zn_handle_t* endpoint1
) {
    return ZN_ERR_UNSUPPORTED;
}

zn_status_t syscall_channel_open(
    zn_handle_t channel,
    size_t num_handles,
    size_t num_bytes,
    __user zn_handle_t** out_handle_buf,
    __user void** out_data_buf
) {
    return ZN_ERR_UNSUPPORTED;
}
