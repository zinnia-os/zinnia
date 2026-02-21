#pragma once

#include <kernel/sched.h>

typedef zn_status_t (*syscall_fn_t)(struct arch_context* ctx);

void syscall_dispatch(struct arch_context* ctx);

zn_status_t syscall_log(struct arch_context* ctx);
zn_status_t syscall_archctl(struct arch_context* ctx);
zn_status_t syscall_page_size(struct arch_context* ctx);

zn_status_t syscall_handle_validate(struct arch_context* ctx);
zn_status_t syscall_handle_drop(struct arch_context* ctx);
zn_status_t syscall_handle_clone(struct arch_context* ctx);

zn_status_t syscall_vas_create(struct arch_context* ctx);

zn_status_t syscall_vmo_create(struct arch_context* ctx);
zn_status_t syscall_vmo_create_phys(struct arch_context* ctx);
zn_status_t syscall_vmo_map(struct arch_context* ctx);

zn_status_t syscall_channel_create(struct arch_context* ctx);
zn_status_t syscall_channel_wait(struct arch_context* ctx);
zn_status_t syscall_channel_read(struct arch_context* ctx);
zn_status_t syscall_channel_write(struct arch_context* ctx);

zn_status_t syscall_thread_create(struct arch_context* ctx);
zn_status_t syscall_thread_start(struct arch_context* ctx);
zn_status_t syscall_thread_exit(struct arch_context* ctx);

zn_status_t syscall_random_get(struct arch_context* ctx);
zn_status_t syscall_random_entropy(struct arch_context* ctx);
