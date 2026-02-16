#ifndef ZINNIA_CHANNEL_H
#define ZINNIA_CHANNEL_H

#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

enum zn_channel_flags {
    // Allow sending messages even if one endpoint is not connected.
    ZN_CHANNEL_NONBLOCK = 1 << 0,
};

// Creates a new channel.
zn_status_t zn_channel_create(enum zn_channel_flags flags, zn_handle_t* endpoint0, zn_handle_t* endpoint1);

// Notifies the peer that there is new data available.
// The kernel only copies the first `num_handles` handles and `num_bytes` bytes.
zn_status_t zn_channel_write(
    zn_handle_t channel,
    zn_handle_t* handles,
    void* bytes,
    size_t num_handles,
    size_t num_bytes
);

// Waits for a new message to appear in the channel.
zn_status_t zn_channel_read(
    zn_handle_t channel,
    zn_handle_t* handles,
    void* bytes,
    size_t num_handles,
    size_t num_bytes,
    size_t* read_handles,
    size_t* read_bytes
);

#ifdef __cplusplus
}
#endif

#endif
