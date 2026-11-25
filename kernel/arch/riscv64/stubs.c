// SPDX-License-Identifier: BSD-3-Clause

#include <lib/errno.h>
#include <lib/irq.h>
#include <lib/panic.h>
#include <lib/percpu.h>
#include <lib/sched.h>
#include <lib/signal.h>
#include <lib/timer.h>
#include <mem/pmap.h>
#include <mem/usercopy.h>
#include <syscalls/syscall.h>
#include <bits/archctl.h>

void arch_pte_shootdown_kick(struct percpu* cpu) {
    panic("riscv64: %s not implemented\n", __func__);
}

errno_t arch_irq_alloc_msi(struct msi_line** out) {
    return ENODEV;
}

errno_t arch_irq_get_gsi(uint32_t gsi, struct irq_line** out) {
    return ENODEV;
}

bool arch_irq_isa_override(
    uint8_t isa_irq,
    uint32_t* out_gsi,
    enum irq_trigger_mode* out_trigger,
    enum irq_polarity* out_polarity
) {
    return false;
}

struct syscall_ret arch_archctl(uintptr_t op, uintptr_t value) {
    return (struct syscall_ret){.error = EINVAL};
}

void arch_syscall_save(const arch_context_t* ctx, arch_syscall_restart_t* out) {
    out->a0 = ctx->a0;
    out->a1 = ctx->a1;
}

void arch_syscall_restart(arch_context_t* ctx, const arch_syscall_restart_t* restart) {
    ctx->sepc -= 4;
    ctx->a0 = restart->a0;
    ctx->a1 = restart->a1;
}

void arch_syscall_set_return(arch_context_t* ctx, uintptr_t value, errno_t error) {
    ctx->a0 = value;
    ctx->a1 = error;
}
