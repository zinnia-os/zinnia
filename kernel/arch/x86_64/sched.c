// SPDX-License-Identifier: BSD-3-Clause

#include <arch/x86_64/apic.h>
#include <arch/x86_64/asm.h>
#include <arch/x86_64/defs.h>
#include <arch/x86_64/gdt.h>
#include <arch/x86_64/idt.h>
#include <arch/x86_64/percpu.h>
#include <lib/assert.h>
#include <lib/compiler.h>
#include <lib/errno.h>
#include <lib/irq.h>
#include <lib/percpu.h>
#include <lib/sched.h>
#include <mem/alloc.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

// The task frame consists of registers that the C ABI marks as callee-saved.
// If we don't save them, these registers are lost during a context switch.
struct task_frame {
    uint64_t rbx, rbp, r12, r13, r14, r15, rip;
};

struct task* perform_switch(uint64_t* old_rsp, uint64_t new_rsp, struct task* prev);
void task_entry_thunk();

DEFINE_ASM_FUNC(
    perform_switch,
    "sub rsp, 0x30\n" // Make room for all regs (except RIP).
    "mov [rsp + %c[rbx]], rbx\n"
    "mov [rsp + %c[rbp]], rbp\n"
    "mov [rsp + %c[r12]], r12\n"
    "mov [rsp + %c[r13]], r13\n"
    "mov [rsp + %c[r14]], r14\n"
    "mov [rsp + %c[r15]], r15\n"
    "mov [rdi], rsp\n" // rdi = old_rsp
    "mov rsp, rsi\n"   // rsi = new_rsp
    "mov rbx, [rsp + %c[rbx]]\n"
    "mov rbp, [rsp + %c[rbp]]\n"
    "mov r12, [rsp + %c[r12]]\n"
    "mov r13, [rsp + %c[r13]]\n"
    "mov r14, [rsp + %c[r14]]\n"
    "mov r15, [rsp + %c[r15]]\n"
    "add rsp, 0x30\n"
    "mov rax, rdx\n"
    "ret\n", // This will conveniently move us to the RIP we put at this stack entry.
    [rbx] "i"(offsetof(struct task_frame, rbx)),
    [rbp] "i"(offsetof(struct task_frame, rbp)),
    [r12] "i"(offsetof(struct task_frame, r12)),
    [r13] "i"(offsetof(struct task_frame, r13)),
    [r14] "i"(offsetof(struct task_frame, r14)),
    [r15] "i"(offsetof(struct task_frame, r15))
);

// Calls task_entry with the correct parameters.
DEFINE_ASM_FUNC(
    task_entry_thunk,
    "mov rdi, rbx\n"
    "mov rsi, r12\n"
    "mov rdx, rax\n"
    "jmp task_entry\n"
);

struct task* arch_sched_switch(struct task* from, struct task* to) {
    struct arch_percpu* cpu = percpu_ref(arch_data);
    arch_task_context_t* from_ctx = &from->context;
    arch_task_context_t* to_ctx = &to->context;

    cpu->tss.rsp0 = kernel_stack_top(&to->kernel_stack);

    if (from_ctx->fpu_region != nullptr) {
        cpu->fpu_save(from_ctx->fpu_region);
        from_ctx->ds = asm_read_ds();
        from_ctx->es = asm_read_es();
        from_ctx->fs = asm_read_fs();
        from_ctx->gs = asm_read_gs();
        from_ctx->fs_base = asm_rdmsr(MSR_FS_BASE);
        from_ctx->gs_base = asm_rdmsr(MSR_KERNEL_GS_BASE);
    }

    if (to_ctx->fpu_region != nullptr) {
        cpu->fpu_restore(to_ctx->fpu_region);
        asm_write_ds(to_ctx->ds);
        asm_write_es(to_ctx->es);
        asm_write_fs(to_ctx->fs);

        // If we have to change the GS segment we need to reload the MSR, otherwise we lose its value.
        if (to_ctx->gs != asm_read_gs()) {
            uint64_t percpu = asm_rdmsr(MSR_GS_BASE);
            asm_write_gs(to_ctx->gs);
            asm_wrmsr(MSR_GS_BASE, percpu);
        }

        asm_wrmsr(MSR_FS_BASE, to_ctx->fs_base);
        asm_wrmsr(MSR_KERNEL_GS_BASE, to_ctx->gs_base);
    }

    return perform_switch(&from_ctx->rsp, to_ctx->rsp, from);
}

constexpr size_t FPU_FCW_OFFSET = 0;
constexpr size_t FPU_MXCSR_OFFSET = 24;
constexpr uint16_t FPU_DEFAULT_FCW = 0x37F;
constexpr uint32_t FPU_DEFAULT_MXCSR = 0x1F80;

errno_t arch_task_init(arch_task_context_t* context, void* entry, uintptr_t arg0, uintptr_t stack_start, bool is_user) {
    struct arch_percpu* cpu = percpu_ref(arch_data);

    assert((stack_start & 0xF) == 0, "Task stack is not 16-byte aligned!");

    // Prepare a dummy stack with an entry point function to return to.
    struct task_frame* frame = (struct task_frame*)(stack_start - sizeof(uint64_t)) - 1;
    frame->rbx = (uint64_t)entry;
    frame->r12 = arg0;
    frame->rip = (uint64_t)task_entry_thunk;
    context->rsp = (uint64_t)frame;

    // Zero the return address to stop backtraces.
    *(uint64_t*)(stack_start - sizeof(uint64_t)) = 0;

    if (is_user) {
        // Allocate FPU space.
        const size_t fpu_size = cpu->fpu_size;
        uintptr_t fpu_base = vm_alloc(fpu_size);
        if (!fpu_base)
            return ENOMEM;
        phys_t pages;
        errno_t s = mem_phys_alloc(DIV_CEIL(fpu_size, arch_page_size()), 0, &pages);
        if (s) {
            vm_free(fpu_base, fpu_size);
            return s;
        }
        for (size_t i = 0; i < fpu_size; i += arch_page_size())
            pmap_map(&kernel_vma.pmap, fpu_base + i, pages + i, PTE_WRITE | PTE_READ, CACHE_WRITE_BACK);
        context->fpu_region = (void*)fpu_base;
        context->fpu_phys = pages;

        const uint16_t fcw = FPU_DEFAULT_FCW;
        const uint32_t mxcsr = FPU_DEFAULT_MXCSR;
        memcpy((uint8_t*)fpu_base + FPU_FCW_OFFSET, &fcw, sizeof(fcw));
        memcpy((uint8_t*)fpu_base + FPU_MXCSR_OFFSET, &mxcsr, sizeof(mxcsr));

        context->ds = asm_read_ds();
        context->es = asm_read_es();
        context->fs = asm_read_fs();
        context->gs = asm_read_gs();
        context->fs_base = asm_rdmsr(MSR_FS_BASE);
        context->gs_base = asm_rdmsr(MSR_KERNEL_GS_BASE);
    }

    return 0;
}

void arch_task_fini(arch_task_context_t* context) {
    if (context->fpu_region == nullptr)
        return;

    const size_t page_size = arch_page_size();
    const size_t fpu_size = percpu_ref(arch_data)->fpu_size;
    const uintptr_t fpu_base = (uintptr_t)context->fpu_region;
    for (size_t i = 0; i < fpu_size; i += page_size)
        pmap_unmap(&kernel_vma.pmap, fpu_base + i);
    pmap_shootdown(&kernel_vma.pmap);
    mem_phys_free(context->fpu_phys, DIV_CEIL(fpu_size, page_size));
    vm_free(fpu_base, fpu_size);

    context->fpu_region = nullptr;
}

void arch_task_fork(arch_task_context_t* dst) {
    if (dst->fpu_region)
        percpu_ref(arch_data)->fpu_save(dst->fpu_region);
}

void arch_context_init_user(arch_context_t* ctx, uintptr_t ip, uintptr_t sp) {
    *ctx = (arch_context_t){
        .rip = ip,
        .rsp = sp,
        .rflags = 0x202,
        .cs = offsetof(struct gdt, user_code64) | CPL_USER,
        .ss = offsetof(struct gdt, user_data) | CPL_USER,
    };
}

void arch_sched_kick(struct percpu* cpu) {
    if (!cpu->online)
        return;

    // We need to lock IRQs here because the xAPIC ICR takes two writes,
    // which has to be completed without an interrupt in between.
    irq_lock();
    lapic_send_ipi(
        &percpu_ref(arch_data)->lapic,
        percpu_ref_on(cpu, arch_data)->lapic.id,
        IDT_IPI_RESCHED,
        APIC_DELIVERY_FIXED,
        APIC_DEST_PHYSICAL,
        APIC_LEVEL_ASSERT,
        APIC_TRIGGER_EDGE
    );
    irq_unlock();
}

void arch_sched_jump_to_context(arch_context_t* context) {
    asm volatile(
        "mov rsp, %0\n"
        "jmp %1\n" ::"r"(context),
        "r"(interrupt_return)
    );
    __builtin_unreachable();
}

void arch_sched_jump_to_user(uintptr_t ip, uintptr_t sp, uintptr_t arg) {
    arch_context_t context = {
        .rip = ip,
        .rsp = sp,
        .rdi = arg,
        .rflags = 0x202,
        .cs = offsetof(struct gdt, user_code64) | CPL_USER,
        .ss = offsetof(struct gdt, user_data) | CPL_USER,
    };

    uintptr_t percpu = asm_rdmsr(MSR_GS_BASE);
    asm volatile(
        "mov ds, %0\n"
        "mov es, %0\n"
        "mov fs, %0\n"
        "mov gs, %0\n"
        :
        : "a"(0)
        : "memory"
    );

    asm_wrmsr(MSR_FS_BASE, 0);
    asm_wrmsr(MSR_GS_BASE, percpu);
    asm_wrmsr(MSR_KERNEL_GS_BASE, 0);

    arch_sched_jump_to_context(&context);
}
