// SPDX-License-Identifier: BSD-3-Clause

#include <arch/riscv64/defs.h>
#include <arch/riscv64/sbi.h>
#include <arch/riscv64/trap.h>
#include <lib/compiler.h>
#include <lib/irq.h>
#include <lib/panic.h>
#include <lib/percpu.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <lib/signal.h>
#include <lib/timer.h>
#include <lib/unwind.h>
#include <lib/utils.h>
#include <mem/pmap.h>
#include <mem/usercopy.h>
#include <mem/vma.h>
#include <syscalls/syscall.h>
#include <bits/preempt.h>
#include <stddef.h>

static_assert(offsetof(arch_context_t, x) == 0);
static_assert(offsetof(arch_context_t, sepc) == 0);
static_assert(offsetof(arch_context_t, sstatus) == 32 * sizeof(uint64_t));

// The context needs to be aligned to 16 bytes before entering the trap handler.
constexpr size_t FRAME_SIZE = ROUND_UP(sizeof(arch_context_t), 16);

void trap_handler(arch_context_t* ctx) {
    uint64_t scause, stval;
    asm volatile("csrr %0, scause" : "=r"(scause));
    asm volatile("csrr %0, stval" : "=r"(stval));
    struct percpu* cpu = percpu_get();
    const bool from_user = !(ctx->sstatus & SSTATUS_SPP);

    if (scause == (SCAUSE_INTERRUPT | IRQ_S_TIMER)) {
        sbi_set_timer(UINT64_MAX);
        arch_sched_preempt_disable();
        timer_interrupt();
        if (arch_sched_preempt_enable() && (cpu->sched.need_resched || cpu->sched.owe_preempt))
            sched_reschedule(&cpu->sched);
    } else if (scause == EXC_ECALL_U) {
        ctx->sepc += 4;
        syscall_dispatch(ctx);
        return;
    } else if (scause == EXC_FETCH_PAGE_FAULT || scause == EXC_LOAD_PAGE_FAULT || scause == EXC_STORE_PAGE_FAULT) {
        const struct vma_fault_info info = {
            .ip = ctx->sepc,
            .addr = stval,
            .caused_by_user = from_user,
            .caused_by_write = scause == EXC_STORE_PAGE_FAULT,
            .caused_by_fetch = scause == EXC_FETCH_PAGE_FAULT,
        };

        struct task* current = cpu->sched.current;
        const bool irqs_were_on = ctx->sstatus & SSTATUS_SPIE;
        if (irqs_were_on)
            irq_set_state(true);

        const errno_t fault_status = current && current->space ? vma_page_fault(current->space, &info) : EFAULT;

        if (irqs_were_on)
            irq_set_state(false);

        if (fault_status) {
            struct usercopy_region* region = current ? current->usercopy_region : nullptr;
            if (region && ctx->sepc >= (uintptr_t)region->start_ip && ctx->sepc < (uintptr_t)region->end_ip) {
                ctx->sepc = (uintptr_t)region->fault_ip;
            } else if (from_user) {
                const struct sig_info fault = {
                    .code = fault_status == EACCES ? __SEGV_ACCERR : __SEGV_MAPERR,
                    .addr = info.addr,
                };
                signal_force(current, SIGSEGV, &fault);
            } else {
                phys_t phys;
                const bool present =
                    current && current->space && !pmap_extract(&current->space->pmap, info.addr, &phys);
                panic(
                    "Unrecoverable page fault. Attempted to %s a %s page at %#lx (IP: %#lx)\n",
                    info.caused_by_write       ? "write to"
                        : info.caused_by_fetch ? "execute on"
                                               : "read from",
                    present ? "present" : "non-present",
                    info.addr,
                    info.ip
                );
            }
        }
    } else {
        panic("Unhandled trap: scause = %#lx, stval = %#lx, sepc = %#lx\n", scause, stval, ctx->sepc);
    }

    struct task* task = cpu->sched.current;
    if (from_user && task && task->process != process_kernel() && signal_pending(task)) {
        irq_set_state(true);
        signal_deliver(ctx, nullptr);
        irq_set_state(false);
    }
}

#define IRP_GPRS_ALL \
    "1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, " \
    "29, 30, 31\n"

#define IRP_GPRS_NO_SP_TP \
    "1, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31\n"

DEFINE_ASM_ALIGNED(
    stvec_entry,
    4,
    // If we're coming out of userspace, save that tp in sscratch and restore ours.
    "    csrrw tp, sscratch, tp\n"
    "    bnez tp, 1f\n"
    "    csrr tp, sscratch\n"
    "    sd sp, %c[scratch](tp)\n"
    "    j 2f\n"
    "1:\n"
    "    sd sp, %c[scratch](tp)\n"
    "    ld sp, %c[kstack](tp)\n"
    "2:\n"
    "    addi sp, sp, -%c[size]\n"
    // Save all registers except SP and TP, we'll do that after.
    ".irp n, " IRP_GPRS_NO_SP_TP
    "    sd x\\n, \\n * 8(sp)\n"
    ".endr\n"
    // Get the user SP back and write it to the frame.
    "    ld t0, %c[scratch](tp)\n"
    "    sd t0, 2 * 8(sp)\n"
    // Same with TP.
    "    csrrw t0, sscratch, zero\n"
    "    sd t0, 4 * 8(sp)\n"
    // Get sepc.
    "    csrr t0, sepc\n"
    "    sd t0, %c[sepc](sp)\n"
    "    li t1, %c[sum]\n"
    // Get sstatus.
    "    csrrc t0, sstatus, t1\n"
    "    sd t0, %c[sstatus](sp)\n"
    ".cfi_signal_frame\n"
    ".cfi_return_column %c[pc_reg]\n"
    ".cfi_def_cfa sp, %c[size]\n"
    ".irp n, " IRP_GPRS_ALL
    ".cfi_offset \\n, \\n * 8 - %c[size]\n"
    ".endr\n"
    ".cfi_offset %c[pc_reg], %c[sepc] - %c[size]\n"
    "    li s0, 0\n"
    // First argument is the frame on the stack.
    "    mv a0, sp\n"
    "    call %c[handler]\n"
    "    j trap_return\n",
    [scratch] "i"(offsetof(struct percpu, user_stack)),
    [kstack] "i"(offsetof(struct percpu, kernel_stack)),
    [size] "i"(FRAME_SIZE),
    [sepc] "i"(offsetof(arch_context_t, sepc)),
    [sstatus] "i"(offsetof(arch_context_t, sstatus)),
    [sum] "i"(SSTATUS_SUM),
    [pc_reg] "i"(UNWIND_REG_PC),
    [handler] "i"(trap_handler)
);

DEFINE_ASM_FUNC(
    trap_return,
    // Restore sepc.
    "    ld t0, %c[sepc](sp)\n"
    "    csrw sepc, t0\n"
    // Restore sstatus.
    "    ld t0, %c[sstatus](sp)\n"
    "    csrw sstatus, t0\n"
    "    andi t0, t0, %c[spp]\n"
    // If we came from userspace, save our TP again and load the user IP.
    "    bnez t0, 1f\n"
    "    csrw sscratch, tp\n"
    "    ld tp, 4 * 8(sp)\n"
    "1:\n"
    // Restore the rest.
    ".irp n, " IRP_GPRS_NO_SP_TP
    "    ld x\\n, \\n * 8(sp)\n"
    ".endr\n"
    "    ld sp, 2 * 8(sp)\n"
    "    sret\n",
    [sepc] "i"(offsetof(arch_context_t, sepc)),
    [sstatus] "i"(offsetof(arch_context_t, sstatus)),
    [spp] "i"(SSTATUS_SPP)
);
