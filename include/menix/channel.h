#ifndef MENIX_CHANNEL_H
#define MENIX_CHANNEL_H

#include <menix/handle.h>
#include <menix/status.h>
#include <stddef.h>

enum menix_channel_flags {
    // Allow sending messages even if one endpoint is not connected.
    MENIX_CHANNEL_NONBLOCK = 1 << 0,
    MENIX_
};

// Creates a new channel.
menix_status_t menix_channel_create(
    enum menix_channel_flags flags,
    menix_handle_t* endpoint0,
    menix_handle_t* endpoint1
);

// Maps the message buffer in the address space and returns its base address.
// There may only be one message buffer per channel and process.
menix_status_t menix_channel_open(
    menix_handle_t channel,
    size_t num_handles,
    size_t num_bytes,
    menix_handle_t** out_handle_buf,
    void** out_data_buf
);

// Waits for a new message to appear in the channel buffer.
menix_status_t menix_channel_read(menix_handle_t channel);

// Writes a message to the
menix_status_t menix_channel_write(menix_handle_t channel);

#endif
