// SPDX-License-Identifier: BSD-3-Clause

#include <arch/x86_64/apic.h>
#include <arch/x86_64/defs.h>
#include <arch/x86_64/gdt.h>
#include <arch/x86_64/idt.h>
#include <arch/x86_64/percpu.h>
#include <lib/assert.h>
#include <lib/compiler.h>
#include <lib/irq.h>
#include <lib/panic.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <lib/signal.h>
#include <lib/timer.h>
#include <lib/utils.h>
#include <mem/pmap.h>
#include <mem/usercopy.h>
#include <mem/vma.h>
#include <bits/preempt.h>
#include <stdcountof.h>
#include <stddef.h>
#include <stdint.h>

struct [[__packed]] idt_entry {
    uint16_t base0;
    uint16_t selector;
    uint8_t ist;
    uint8_t attrib;
    uint16_t base1;
    uint32_t base2;
    uint32_t reserved;
};

static_assert(sizeof(struct idt_entry) == 16);

struct [[__packed]] idtr {
    uint16_t limit;
    struct idt_entry* base;
};

static_assert(sizeof(struct idtr) == 10);

void interrupt_handler(arch_context_t* ctx) {
    struct percpu* cpu = percpu_get();
    struct arch_percpu* arch_cpu = percpu_ref(arch_data);
    uint64_t isr = ctx->isr;
    const bool from_user = (ctx->cs & CPL_USER) == CPL_USER;

    switch (isr) {
    case IDT_PF:
        uint64_t cr2;
        asm volatile("mov %0, cr2" : "=r"(cr2));

        const struct vma_fault_info info = {
            .ip = ctx->rip,
            .addr = cr2,
            .caused_by_user = from_user,
            .caused_by_write = ctx->error & (1 << 1),
            .caused_by_fetch = ctx->error & (1 << 4),
            .page_was_present = ctx->error & (1 << 0),
        };

        struct task* current = cpu->sched.current;
        const bool irqs_were_on = ctx->rflags & RFLAGS_IF;
        if (irqs_were_on)
            irq_set_state(true);

        const errno_t fault_status = current && current->space ? vma_page_fault(current->space, &info) : EFAULT;

        if (irqs_were_on)
            irq_set_state(false);

        if (!fault_status)
            break;

        if (current && current->usercopy_region) {
            struct usercopy_region* region = current->usercopy_region;
            if (ctx->rip >= (uintptr_t)region->start_ip && ctx->rip < (uintptr_t)region->end_ip) {
                ctx->rip = (uintptr_t)region->fault_ip;
                break;
            }
        }

        if (from_user) {
            const struct sig_info fault = {
                .code = fault_status == EACCES ? __SEGV_ACCERR : __SEGV_MAPERR,
                .addr = info.addr,
            };
            signal_force(current, SIGSEGV, &fault);
            break;
        }

        panic(
            "Unrecoverable page fault. Attempted to %s a %s page at %#lx (IP: %#lx)\n",
            info.caused_by_write       ? "write to"
                : info.caused_by_fetch ? "execute on"
                                       : "read from",
            info.page_was_present ? "present" : "non-present",
            info.addr,
            info.ip
        );
    case IDT_SPURIOUS:
        break;
    case IDT_IPI_RESCHED:
        lapic_eoi(&arch_cpu->lapic);
        arch_sched_preempt_disable();
        if (arch_sched_preempt_enable())
            sched_reschedule(&cpu->sched);
        break;
    case IDT_IPI_SHOOTDOWN:
        lapic_eoi(&arch_cpu->lapic);
        pmap_shootdown_handle();
        break;
    case IDT_TIMER:
        lapic_eoi(&arch_cpu->lapic);
        arch_sched_preempt_disable();
        timer_interrupt();
        if (arch_sched_preempt_enable() && (cpu->sched.need_resched || cpu->sched.owe_preempt))
            sched_reschedule(&cpu->sched);
        break;
    default:
        if (isr < 0x20) {
            // Map the exception to a signal.
            signal_t sig = 0;
            int code = __SI_KERNEL;
            switch (isr) {
            case IDT_DE:
                sig = SIGFPE;
                code = __FPE_INTDIV;
                break;
            case IDT_DB:
            case IDT_BP:
                sig = SIGTRAP;
                code = __TRAP_BRKPT;
                break;
            case IDT_UD:
                sig = SIGILL;
                code = __ILL_ILLOPC;
                break;
            case IDT_MF:
            case IDT_XM:
                sig = SIGFPE;
                break;
            case IDT_AC:
                sig = SIGBUS;
                code = __BUS_ADRALN;
                break;
            case IDT_OF:
            case IDT_BR:
            case IDT_NM:
            case IDT_SS:
            case IDT_GP:
                sig = SIGSEGV;
                break;
            }
            if (from_user && sig) {
                const struct sig_info fault = {.code = code, .addr = ctx->rip};
                signal_force(cpu->sched.current, sig, &fault);
                break;
            }
            panic("Unhandled exception %lu at %#lx, error: %#lx\n", isr, ctx->rip, ctx->error);
        }
        assert(isr - 0x20 < countof(arch_cpu->irq_lines), "Unhandled interrupt on ISR %lu", isr);
        struct irq_line* line = arch_cpu->irq_lines[isr - 0x20];
        assert(line, "Unhandled interrupt on ISR %lu", isr);
        arch_sched_preempt_disable();
        irq_line_raise(line);
        if (arch_sched_preempt_enable() && cpu->sched.owe_preempt)
            sched_reschedule(&cpu->sched);
    }

    // Deliver pending signals before we return to user space.
    struct task* task = percpu_get()->sched.current;
    if (from_user && task && task->process != process_kernel() && signal_pending(task)) {
        irq_set_state(true);
        signal_deliver(ctx, nullptr);
        irq_set_state(false);
    }
}

#define CS_OFFSET (sizeof(arch_context_t) - sizeof(uint64_t) - offsetof(arch_context_t, cs))

void interrupt_stub_internal();

DEFINE_ASM_FUNC(
    interrupt_return,
    "pop r15\n"
    "pop r14\n"
    "pop r13\n"
    "pop r12\n"
    "pop r11\n"
    "pop r10\n"
    "pop r9\n"
    "pop r8\n"
    "pop rsi\n"
    "pop rdi\n"
    "pop rbp\n"
    "pop rdx\n"
    "pop rcx\n"
    "pop rbx\n"
    "pop rax\n"
    // Change GS back if we came from user mode.
    "cmp word ptr [rsp + %c0], %c1\n"
    "je 2f\n"
    "swapgs\n"
    "2:\n"
    // Skip .error and .isr fields.
    "add rsp, 0x10\n"
    "iretq\n",
    "i"(CS_OFFSET),
    "i"(offsetof(struct gdt, kernel_code64))
);

#define CTX_CFI(reg)         ".cfi_offset " #reg ", %c[cfi_" #reg "]\n"
#define CTX_CFI_OPERAND(reg) [cfi_##reg] "i"((long)offsetof(arch_context_t, reg) - (long)sizeof(arch_context_t))

DEFINE_ASM_FUNC(
    interrupt_stub_internal,
    // Load the kernel GS base if we're coming from user space.
    "cmp word ptr [rsp + %c0], %c1\n"
    "je 2f\n"
    "swapgs\n"
    "2:\n"
    "push rax\n"
    "push rbx\n"
    "push rcx\n"
    "push rdx\n"
    "push rbp\n"
    "push rdi\n"
    "push rsi\n"
    "push r8\n"
    "push r9\n"
    "push r10\n"
    "push r11\n"
    "push r12\n"
    "push r13\n"
    "push r14\n"
    "push r15\n"
    "cld\n"
    // Zero out the base pointer since we can't trust it.
    "xor rbp, rbp\n"
    // clang-format off
    ".cfi_signal_frame\n"
    ".cfi_def_cfa rsp, %c[ctx_size]\n"
    CTX_CFI(rax)
    CTX_CFI(rbx)
    CTX_CFI(rcx)
    CTX_CFI(rdx)
    CTX_CFI(rsi)
    CTX_CFI(rdi)
    CTX_CFI(rbp)
    CTX_CFI(r8)
    CTX_CFI(r9)
    CTX_CFI(r10)
    CTX_CFI(r11)
    CTX_CFI(r12)
    CTX_CFI(r13)
    CTX_CFI(r14)
    CTX_CFI(r15)
    CTX_CFI(rsp)
    CTX_CFI(rip)
    // clang-format on
    // Load the frame as first argument.
    "mov rdi, rsp\n"
    "call %c2\n"
    "jmp interrupt_return\n",
    "i"(CS_OFFSET),
    "i"(offsetof(struct gdt, kernel_code64)),
    "i"(interrupt_handler),
    [ctx_size] "i"(sizeof(arch_context_t)),
    CTX_CFI_OPERAND(rax),
    CTX_CFI_OPERAND(rbx),
    CTX_CFI_OPERAND(rcx),
    CTX_CFI_OPERAND(rdx),
    CTX_CFI_OPERAND(rsi),
    CTX_CFI_OPERAND(rdi),
    CTX_CFI_OPERAND(rbp),
    CTX_CFI_OPERAND(r8),
    CTX_CFI_OPERAND(r9),
    CTX_CFI_OPERAND(r10),
    CTX_CFI_OPERAND(r11),
    CTX_CFI_OPERAND(r12),
    CTX_CFI_OPERAND(r13),
    CTX_CFI_OPERAND(r14),
    CTX_CFI_OPERAND(r15),
    CTX_CFI_OPERAND(rsp),
    CTX_CFI_OPERAND(rip)
);

extern void (*const interrupt_stubs[256])();

[[__used]]
static void define_interrupt_stubs() {
    asm volatile(
        ".pushsection .text.interrupt_stubs, \"ax\", @progbits\n"
        ".macro interrupt_stub n\n"
        ".Linterrupt_stub_\\n:\n"
        ".if (\\n == 8 || (\\n >= 10 && \\n <= 14) || \\n == 17 || \\n == 21 || \\n == 29 || \\n == 30)\n"
        ".else\n"
        "push 0\n"
        ".endif\n"
        "push \\n\n"
        "jmp interrupt_stub_internal\n"
        ".endm\n"
        ".macro interrupt_stub_entry n\n"
        ".quad .Linterrupt_stub_\\n\n"
        ".endm\n"
        ".altmacro\n"
        ".set .Lvector, 0\n"
        ".rept 256\n"
        "interrupt_stub %.Lvector\n"
        ".set .Lvector, .Lvector + 1\n"
        ".endr\n"
        ".popsection\n"
        ".pushsection .data.rel.ro.interrupt_stubs, \"aw\", @progbits\n"
        ".balign 8\n"
        ".global interrupt_stubs\n"
        ".type interrupt_stubs, @object\n"
        "interrupt_stubs:\n"
        ".set .Lvector, 0\n"
        ".rept 256\n"
        "interrupt_stub_entry %.Lvector\n"
        ".set .Lvector, .Lvector + 1\n"
        ".endr\n"
        ".size interrupt_stubs, . - interrupt_stubs\n"
        ".noaltmacro\n"
        ".purgem interrupt_stub\n"
        ".purgem interrupt_stub_entry\n"
        ".popsection\n"
    );
}

static struct idt_entry idt[256] = {};

void idt_init() {
    for (size_t n = 0; n < countof(idt); n++) {
        uint64_t addr = (uint64_t)interrupt_stubs[n];
        idt[n].base0 = addr & 0xFFFF;
        idt[n].base1 = (addr >> 16) & 0xFFFF;
        idt[n].base2 = (addr >> 32) & 0xFFFF'FFFF;
        idt[n].ist = 0;
        idt[n].attrib = (1 << 7) | 0xE;
        idt[n].selector = offsetof(struct gdt, kernel_code64);
        idt[n].reserved = 0;
    }
    idt[IDT_BP].attrib |= CPL_USER << 5;
}

void idt_load() {
    struct idtr idtr = {
        .limit = sizeof(idt) - 1,
        .base = idt,
    };

    asm volatile("lidt %0" ::"m"(idtr));
}
