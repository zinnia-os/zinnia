// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <bits/sched.h>
#include <stddef.h>
#include <stdint.h>
#include <zinnia/ucontext.h>

struct sig_delivery;

size_t arch_signal_frame_size();
errno_t arch_signal_setup_frame(arch_context_t* ctx, const struct sig_delivery* delivery);
errno_t arch_signal_restore_context(arch_context_t* ctx, const __ucontext_t* uc, uintptr_t uc_addr);
void arch_syscall_save(const arch_context_t* ctx, arch_syscall_restart_t* out);
void arch_syscall_restart(arch_context_t* ctx, const arch_syscall_restart_t* restart);
void arch_syscall_set_return(arch_context_t* ctx, uintptr_t value, errno_t error);
