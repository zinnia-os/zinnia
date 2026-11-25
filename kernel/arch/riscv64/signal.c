// SPDX-License-Identifier: BSD-3-Clause

#include <lib/errno.h>
#include <lib/sched.h>
#include <stddef.h>
#include <zinnia/ucontext.h>

size_t arch_signal_frame_size() {
    // TODO
    return 0;
}

errno_t arch_signal_setup_frame(arch_context_t* ctx, const struct sig_delivery* delivery) {
    // TODO
    return ENOSYS;
}

errno_t arch_signal_restore_context(arch_context_t* ctx, const __ucontext_t* uc, uintptr_t uc_addr) {
    // TODO
    return ENOSYS;
}
