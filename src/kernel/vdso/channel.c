#include <zinnia/channel.h>
#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <vdso/common.h>
#include <vdso/syscall_stubs.h>

VDSO_FUNC(zn_status_t, zn_channel_create, enum zn_channel_flags flags, zn_handle_t* endpoint0, zn_handle_t* endpoint1) {
    return zn_syscall3(flags, endpoint0, endpoint1, ZN_SYSCALL_CHANNEL_CREATE);
}

VDSO_FUNC(
    zn_status_t,
    zn_channel_write,
    zn_handle_t channel,
    zn_handle_t* handles,
    void* bytes,
    size_t num_handles,
    size_t num_bytes
) {
    return zn_syscall5(channel, handles, bytes, num_handles, num_bytes, ZN_SYSCALL_CHANNEL_WRITE);
}

VDSO_FUNC(
    zn_status_t,
    zn_channel_read,
    zn_handle_t channel,
    zn_handle_t* handles,
    void* bytes,
    size_t num_handles,
    size_t num_bytes,
    size_t* read_handles,
    size_t* read_bytes
) {
    return zn_syscall7(
        channel,
        handles,
        bytes,
        num_handles,
        num_bytes,
        read_handles,
        read_bytes,
        ZN_SYSCALL_CHANNEL_READ
    );
}
