#include <zinnia/status.h>
#include <common/compiler.h>
#include <kernel/alloc.h>
#include <kernel/percpu.h>
#include <kernel/sched.h>
#include <kernel/virt.h>
#include <bits/sched.h>
#include <stddef.h>
#include <stdint.h>
#include <x86_64/asm.h>
#include <x86_64/defs.h>
#include <x86_64/gdt.h>
#include <x86_64/idt.h>

// The task frame consists of registers that the C ABI marks as callee-saved.
// If we don't save them, these registers are lost during a context switch.
// The order of these fields is important.
struct task_frame {
    uint64_t rbx, rbp, r12, r13, r14, r15, rip;
};

[[__naked]]
static void perform_switch(uint64_t* old_rsp, uint64_t new_rsp) {
    asm volatile(
        "sub rsp, 0x30\n" // Make room for all regs (except RIP).
        "mov [rsp + %c0], rbx\n"
        "mov [rsp + %c1], rbp\n"
        "mov [rsp + %c2], r12\n"
        "mov [rsp + %c3], r13\n"
        "mov [rsp + %c4], r14\n"
        "mov [rsp + %c5], r15\n"
        "mov [rdi], rsp\n" // rdi = old_rsp
        "mov rsp, rsi\n"   // rsi = new_rsp
        "mov rbx, [rsp + %c0]\n"
        "mov rbp, [rsp + %c1]\n"
        "mov r12, [rsp + %c2]\n"
        "mov r13, [rsp + %c3]\n"
        "mov r14, [rsp + %c4]\n"
        "mov r15, [rsp + %c5]\n"
        "add rsp, 0x30\n"
        "call %c6\n"
        "ret" // This will conveniently move us to the RIP we put at this stack entry.
        :
        : "i"(offsetof(struct task_frame, rbx)),
          "i"(offsetof(struct task_frame, rbp)),
          "i"(offsetof(struct task_frame, r12)),
          "i"(offsetof(struct task_frame, r13)),
          "i"(offsetof(struct task_frame, r14)),
          "i"(offsetof(struct task_frame, r15)),
          "i"(irq_unlock)
        : "memory"
    );
}

void arch_sched_switch(struct task* from, struct task* to) {
    struct arch_percpu* cpu = &percpu_get()->arch;
    struct arch_task_context* from_ctx = &from->context;
    struct arch_task_context* to_ctx = &to->context;

    cpu->tss.rsp0 = to->kernel_stack + KERNEL_STACK_SIZE;

    if (from->user_stack) {
        cpu->fpu_save(from_ctx->fpu_region);
        from_ctx->ds = asm_read_ds();
        from_ctx->es = asm_read_es();
        from_ctx->fs = asm_read_fs();
        from_ctx->gs = asm_read_gs();
        from_ctx->fs_base = asm_rdmsr(MSR_FS_BASE);
        from_ctx->gs_base = asm_rdmsr(MSR_KERNEL_GS_BASE);
    }

    if (to->user_stack) {
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

    perform_switch(&from_ctx->rsp, to_ctx->rsp);
}

[[__naked]]
void task_entry_thunk() {
    asm volatile(
        "mov rdi, rbx\n"
        "mov rsi, r12\n"
        "mov rdx, r13\n"
        "push 0\n"
        "jmp %c0"
        :
        : "i"(task_entry)
        : "memory"
    );
}

zn_status_t arch_task_init(
    struct arch_task_context* context,
    void* entry,
    uintptr_t arg0,
    uintptr_t arg1,
    uintptr_t stack_start,
    bool is_user
) {
    struct arch_percpu* cpu = &percpu_get()->arch;

    // Prepare a dummy stack with an entry point function to return to.
    struct task_frame* frame = (struct task_frame*)stack_start - 1;
    frame->rbx = (uint64_t)entry;
    frame->r12 = arg0;
    frame->r13 = arg1;
    frame->rip = (uint64_t)task_entry_thunk;
    context->rsp = (uint64_t)frame;

    if (is_user) {
        // Allocate FPU space.
        const size_t fpu_size = cpu->fpu_size;
        void* fpu_base = vm_alloc(fpu_size);
        if (!fpu_base)
            return ZN_ERR_NO_MEMORY;
        for (size_t i = 0; i < fpu_size; i += arch_mem_page_size()) {
            phys_t page;
            zn_status_t s = mem_phys_alloc(1, 0, &page);
            if (s) {
                vm_free(fpu_base, fpu_size);
                return s;
            }

            pt_map(&kernel_vas.pt, (uintptr_t)fpu_base, page, PTE_WRITE | PTE_READ, CACHE_WRITE_BACK);
        }
        context->fpu_region = fpu_base;

        context->ds = asm_read_ds();
        context->es = asm_read_es();
        context->fs = asm_read_fs();
        context->gs = asm_read_gs();
        context->fs_base = asm_rdmsr(MSR_FS_BASE);
        context->gs_base = asm_rdmsr(MSR_KERNEL_GS_BASE);
    }

    return ZN_OK;
}

[[__naked]]
void arch_sched_preempt_disable() {
    asm volatile(
        "inc qword ptr gs:%c0\n"
        "ret"
        :
        : "i"(offsetof(struct percpu, sched.preempt_level))
    );
}

[[__naked]]
bool arch_sched_preempt_enable() {
    asm volatile(
        "dec qword ptr gs:%c0\n"
        "setz al\n"
        "movzx eax, al\n"
        "ret\n"
        :
        : "i"(offsetof(struct percpu, sched.preempt_level))
        : "cc"
    );
}

[[__naked]]
void arch_sched_jump_to_context(struct arch_context* context) {
    asm volatile(
        "mov rsp, rdi\n"
        "jmp %c0\n" ::"i"(interrupt_return)
    );
}

void arch_sched_jump_to_user(uintptr_t ip, uintptr_t sp) {
    struct arch_context context = {
        .rip = ip,
        .rsp = sp,
        .rflags = 0x202,
        .cs = offsetof(struct gdt, user_code64) | CPL_USER,
        .ss = offsetof(struct gdt, user_data) | CPL_USER,
    };

    // Clear segment registers.
    uintptr_t percpu = asm_rdmsr(MSR_GS_BASE);
    const uint16_t zero = 0;
    asm volatile(
        "mov ds, %0\n"
        "mov es, %0\n"
        "mov fs, %0\n"
        "mov gs, %0\n"
        :
        : "r"(zero)
    );

    asm_wrmsr(MSR_FS_BASE, 0);
    asm_wrmsr(MSR_GS_BASE, percpu);
    asm_wrmsr(MSR_KERNEL_GS_BASE, 0);

    arch_sched_jump_to_context(&context);
}
