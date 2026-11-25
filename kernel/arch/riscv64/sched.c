// SPDX-License-Identifier: BSD-3-Clause

#include <arch/riscv64/defs.h>
#include <lib/assert.h>
#include <lib/compiler.h>
#include <lib/panic.h>
#include <lib/sched.h>
#include <stddef.h>
#include <string.h>

struct task_frame {
    uint64_t ra;
    uint64_t s[12];
    uint64_t pad;
};

static_assert(sizeof(struct task_frame) % 16 == 0);

static uint64_t user_sstatus() {
    uint64_t sstatus;
    asm volatile("csrr %0, sstatus" : "=r"(sstatus));
    return (sstatus & ~(SSTATUS_SIE | SSTATUS_SPP | SSTATUS_FS_MASK | SSTATUS_SUM)) | SSTATUS_SPIE | SSTATUS_FS_INITIAL;
}

struct task* perform_switch(uint64_t* old_sp, uint64_t new_sp, struct task* prev);
void task_entry_thunk();

DEFINE_ASM_FUNC(
    perform_switch,
    "    addi sp, sp, -%c[size]\n"
    "    sd ra, %c[ra](sp)\n"
    ".irp n, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11\n"
    "    sd s\\n, %c[s] + \\n * 8(sp)\n"
    ".endr\n"
    "    sd sp, 0(a0)\n"
    "    mv sp, a1\n"
    "    ld ra, %c[ra](sp)\n"
    ".irp n, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11\n"
    "    ld s\\n, %c[s] + \\n * 8(sp)\n"
    ".endr\n"
    "    addi sp, sp, %c[size]\n"
    "    mv a0, a2\n"
    "    ret\n",
    [size] "i"(sizeof(struct task_frame)),
    [ra] "i"(offsetof(struct task_frame, ra)),
    [s] "i"(offsetof(struct task_frame, s))
);

DEFINE_ASM_FUNC(
    task_entry_thunk,
    ".cfi_undefined ra\n"
    "    mv a2, a0\n"
    "    mv a0, s1\n"
    "    mv a1, s2\n"
    "    li ra, 0\n"
    "    tail task_entry\n"
);

static void fp_save(struct __mcontext_riscv64_d_state* state) {
    asm volatile(
        ".option push\n"
        ".option arch, +d\n"
        "    csrs sstatus, %[fs]\n"
        ".irp n, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, "
        "27, 28, 29, 30, 31\n"
        "    fsd f\\n, \\n * 8(%[state])\n"
        ".endr\n"
        "    frcsr t0\n"
        "    sw t0, %c[fcsr](%[state])\n"
        ".option pop\n"
        :
        : [state] "r"(state), [fs] "r"(SSTATUS_FS_INITIAL), [fcsr] "i"(offsetof(typeof(*state), fcsr))
        : "t0", "memory"
    );
}

static void fp_restore(const struct __mcontext_riscv64_d_state* state) {
    asm volatile(
        ".option push\n"
        ".option arch, +d\n"
        "    csrs sstatus, %[fs]\n"
        ".irp n, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, "
        "27, 28, 29, 30, 31\n"
        "    fld f\\n, \\n * 8(%[state])\n"
        ".endr\n"
        "    lw t0, %c[fcsr](%[state])\n"
        "    fscsr t0\n"
        ".option pop\n"
        :
        : [state] "r"(state), [fs] "r"(SSTATUS_FS_INITIAL), [fcsr] "i"(offsetof(typeof(*state), fcsr))
        : "t0", "memory"
    );
}

struct task* arch_sched_switch(struct task* from, struct task* to) {
    if (from->context.has_fp)
        fp_save(&from->context.fp);
    if (to->context.has_fp)
        fp_restore(&to->context.fp);
    return perform_switch(&from->context.sp, to->context.sp, from);
}

void arch_sched_kick(struct percpu* cpu) {
    // TODO
    panic("riscv64: %s not implemented\n", __func__);
}

void arch_sched_jump_to_context(arch_context_t* context) {
    asm volatile(
        "csrci sstatus, %c[sie]\n"
        "mv sp, %[ctx]\n"
        "tail trap_return\n"
        :
        : [ctx] "r"(context), [sie] "i"(SSTATUS_SIE)
        : "memory"
    );
    __builtin_unreachable();
}

void arch_sched_jump_to_user(uintptr_t ip, uintptr_t sp, uintptr_t arg) {
    arch_context_t context = {
        .sepc = ip,
        .sp = sp,
        .a0 = arg,
        .sstatus = user_sstatus(),
    };
    arch_sched_jump_to_context(&context);
}

errno_t arch_task_init(arch_task_context_t* context, void* entry, uintptr_t arg0, uintptr_t stack_start, bool is_user) {
    assert((stack_start & 0xF) == 0, "Task stack is not 16-byte aligned!");

    struct task_frame* frame = (struct task_frame*)stack_start - 1;
    *frame = (struct task_frame){
        .ra = (uint64_t)task_entry_thunk,
        .s[1] = (uint64_t)entry,
        .s[2] = arg0,
    };

    *context = (arch_task_context_t){
        .sp = (uint64_t)frame,
        .has_fp = is_user,
    };
    return 0;
}

void arch_task_fini(arch_task_context_t* context) {}

void arch_task_fork(arch_task_context_t* dst) {
    if (dst->has_fp)
        fp_save(&dst->fp);
}

void arch_context_init_user(arch_context_t* ctx, uintptr_t ip, uintptr_t sp) {
    *ctx = (arch_context_t){
        .sepc = ip,
        .sp = sp,
        .sstatus = user_sstatus(),
    };
}
