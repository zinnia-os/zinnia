#include <menix/channel.h>
#include <menix/syscall_numbers.h>
#include "syscall_stubs.h"

menix_status_t menix_channel_create(
    enum menix_channel_flags flags,
    menix_handle_t* endpoint0,
    menix_handle_t* endpoint1
) {
    return syscall3(MENIX_SYSCALL_CHANNEL_CREATE, (arg_t)flags, (arg_t)endpoint0, (arg_t)endpoint1);
}

menix_status_t menix_channel_open(
    menix_handle_t channel,
    size_t num_handles,
    size_t num_bytes,
    menix_handle_t** out_handle_buf,
    void** out_data_buf
) {
    return syscall5(
        MENIX_SYSCALL_CHANNEL_CONNECT,
        (arg_t)channel,
        (arg_t)num_handles,
        (arg_t)num_bytes,
        (arg_t)out_handle_buf,
        (arg_t)out_data_buf
    );
}

menix_status_t menix_channel_write(menix_handle_t channel) {
    return syscall1(MENIX_SYSCALL_CHANNEL_WRITE, (arg_t)channel);
}
