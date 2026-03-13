#include <kernel/assert.h>
#include <kernel/compiler.h>
#include <kernel/irq.h>
#include <kernel/panic.h>
#include <kernel/percpu.h>
#include <kernel/print.h>
#include <kernel/sched.h>
#include <bits/sched.h>
#include <stddef.h>
#include <stdint.h>
#include <x86_64/apic.h>
#include <x86_64/defs.h>
#include <x86_64/gdt.h>

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

void interrupt_handler(struct arch_context* ctx) {
    struct percpu* cpu = percpu_get();
    bool old = irq_set_interrupted(true);

    uint64_t isr = ctx->isr;
    switch (isr) {
    case IDT_PF:
        uint64_t cr2;
        asm volatile("mov %0, cr2" : "=r"(cr2));
        panic("Page fault at %#lx, accessed: %#lx\n", ctx->rip, cr2);
    case IDT_IPI_RESCHED:
        arch_sched_preempt_disable();
        if (arch_sched_preempt_enable()) {
            lapic_eoi(&cpu->arch.lapic);
            sched_reschedule(&cpu->sched);
        }
        break;
    case 0x20 ...(0x20 + ARRAY_SIZE(cpu->arch.irq_lines)):
        struct irq_line* line = cpu->arch.irq_lines[isr - 0x20];
        ASSERT(line, "Unhandled interrupt on ISR %lu\n", isr);
        // TODO: irq_raise(line);
        break;
    default:
        panic("Unhandled exception on ISR %lu\n", isr);
    }

    irq_set_interrupted(old);
}

#define CS_OFFSET (sizeof(struct arch_context) - sizeof(uint64_t) - offsetof(struct arch_context, cs))

[[__naked]]
void interrupt_return() {
    asm volatile(
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
        "iretq\n"
        :
        : "i"(CS_OFFSET), "i"(offsetof(struct gdt, kernel_code64))
    );
}

[[__naked]]
void interrupt_stub_internal() {
    asm volatile(
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
        // Load the frame as first argument.
        "mov rdi, rsp\n"
        "call %c2\n"
        "jmp %c3\n"
        :
        : "i"(CS_OFFSET), "i"(offsetof(struct gdt, kernel_code64)), "i"(interrupt_handler), "i"(interrupt_return)
    );
}

// clang-format off
#define REPEAT_256(M) \
    M(0)   M(1)   M(2)   M(3)   M(4)   M(5)   M(6)   M(7)   \
    M(8)   M(9)   M(10)  M(11)  M(12)  M(13)  M(14)  M(15)  \
    M(16)  M(17)  M(18)  M(19)  M(20)  M(21)  M(22)  M(23)  \
    M(24)  M(25)  M(26)  M(27)  M(28)  M(29)  M(30)  M(31)  \
    M(32)  M(33)  M(34)  M(35)  M(36)  M(37)  M(38)  M(39)  \
    M(40)  M(41)  M(42)  M(43)  M(44)  M(45)  M(46)  M(47)  \
    M(48)  M(49)  M(50)  M(51)  M(52)  M(53)  M(54)  M(55)  \
    M(56)  M(57)  M(58)  M(59)  M(60)  M(61)  M(62)  M(63)  \
    M(64)  M(65)  M(66)  M(67)  M(68)  M(69)  M(70)  M(71)  \
    M(72)  M(73)  M(74)  M(75)  M(76)  M(77)  M(78)  M(79)  \
    M(80)  M(81)  M(82)  M(83)  M(84)  M(85)  M(86)  M(87)  \
    M(88)  M(89)  M(90)  M(91)  M(92)  M(93)  M(94)  M(95)  \
    M(96)  M(97)  M(98)  M(99)  M(100) M(101) M(102) M(103) \
    M(104) M(105) M(106) M(107) M(108) M(109) M(110) M(111) \
    M(112) M(113) M(114) M(115) M(116) M(117) M(118) M(119) \
    M(120) M(121) M(122) M(123) M(124) M(125) M(126) M(127) \
    M(128) M(129) M(130) M(131) M(132) M(133) M(134) M(135) \
    M(136) M(137) M(138) M(139) M(140) M(141) M(142) M(143) \
    M(144) M(145) M(146) M(147) M(148) M(149) M(150) M(151) \
    M(152) M(153) M(154) M(155) M(156) M(157) M(158) M(159) \
    M(160) M(161) M(162) M(163) M(164) M(165) M(166) M(167) \
    M(168) M(169) M(170) M(171) M(172) M(173) M(174) M(175) \
    M(176) M(177) M(178) M(179) M(180) M(181) M(182) M(183) \
    M(184) M(185) M(186) M(187) M(188) M(189) M(190) M(191) \
    M(192) M(193) M(194) M(195) M(196) M(197) M(198) M(199) \
    M(200) M(201) M(202) M(203) M(204) M(205) M(206) M(207) \
    M(208) M(209) M(210) M(211) M(212) M(213) M(214) M(215) \
    M(216) M(217) M(218) M(219) M(220) M(221) M(222) M(223) \
    M(224) M(225) M(226) M(227) M(228) M(229) M(230) M(231) \
    M(232) M(233) M(234) M(235) M(236) M(237) M(238) M(239) \
    M(240) M(241) M(242) M(243) M(244) M(245) M(246) M(247) \
    M(248) M(249) M(250) M(251) M(252) M(253) M(254) M(255)
// clang-format on

#define INTERRUPT_STUB(n) \
    [[__naked]] \
    static void interrupt_stub_##n() { \
        asm volatile( \
            ".if (%c0 == 8 || (%c0 >= 10 && %c0 <= 14) || %c0 == 17 || %c0 == 21 || %c0 == 29 || %c0 == 30)\n" \
            ".else\n" \
            "push 0\n" \
            ".endif\n" \
            "push %c0\n" \
            "jmp %c1\n" \
            : \
            : "i"(n), "i"(interrupt_stub_internal) \
        ); \
    }

#define INTERRUPT_SETUP(n) \
    do { \
        uint64_t addr = (uint64_t)interrupt_stub_##n; \
        idt[n].base0 = addr & 0xFFFF; \
        idt[n].base1 = (addr >> 16) & 0xFFFF; \
        idt[n].base2 = (addr >> 32) & 0xFFFF'FFFF; \
        idt[n].ist = 0; \
        idt[n].attrib = (1 << 7) | 0xE; \
        idt[n].selector = offsetof(struct gdt, kernel_code64); \
        idt[n].reserved = 0; \
    } while (0);

REPEAT_256(INTERRUPT_STUB)

static struct idt_entry idt[256] = {};

void idt_init() {
    REPEAT_256(INTERRUPT_SETUP)
}

void idt_load() {
    struct idtr idtr = {
        .limit = sizeof(idt) - 1,
        .base = idt,
    };

    asm volatile("lidt %0" ::"m"(idtr));
}
