#pragma once

#include <zinnia/channel.h>
#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <zinnia/system.h>
#include <common/compiler.h>
#include <kernel/types.h>
#include <stddef.h>
#include <stdint.h>

zn_status_t syscall_dispatch(reg_t a0, reg_t a1, reg_t a2, reg_t a3, reg_t a4, reg_t a5, reg_t num);

zn_status_t syscall_log(__user const char* msg, size_t len);

zn_status_t syscall_archctl(uint32_t op, size_t value);

zn_status_t syscall_random_bytes(__user void* addr, size_t len);

zn_status_t syscall_page_size(__user size_t* out);

zn_status_t syscall_channel_create(
    enum zn_channel_flags flags,
    __user zn_handle_t* endpoint0,
    __user zn_handle_t* endpoint1
);

zn_status_t syscall_channel_open(
    zn_handle_t channel,
    size_t num_handles,
    size_t num_bytes,
    __user zn_handle_t** out_handle_buf,
    __user void** out_data_buf
);
