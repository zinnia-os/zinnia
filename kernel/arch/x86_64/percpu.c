#include <kernel/percpu.h>
#include <kernel/print.h>
#include <stddef.h>
#include <x86_64/apic.h>
#include <x86_64/asm.h>
#include <x86_64/defs.h>
#include <x86_64/gdt.h>
#include <x86_64/hpet.h>
#include <x86_64/idt.h>
#include <x86_64/syscall.h>
#include <x86_64/tsc.h>

void percpu_bsp_init() {
    asm_wrmsr(MSR_GS_BASE, (uint64_t)&percpu_bsp);
    asm_wrmsr(MSR_FS_BASE, 0);
    asm_wrmsr(MSR_KERNEL_GS_BASE, 0);

    pic_disable();
    gdt_load();
    idt_init();
    idt_load();
}

union ia32_star {
    uint64_t value;
    struct {
        uint32_t rsvd;
        uint16_t kernel_cs_ss;
        uint16_t user_cs_ss;
    };
};

static void setup_cpu(struct percpu* cpu) {
    asm_wrmsr(MSR_GS_BASE, (uint64_t)cpu);
    asm_wrmsr(MSR_FS_BASE, 0);
    asm_wrmsr(MSR_KERNEL_GS_BASE, 0);

    gdt_load();
    idt_load();

    // Syscall extension.
    asm_wrmsr(MSR_EFER, asm_rdmsr(MSR_EFER) | MSR_EFER_SCE);
    union ia32_star star = {0};
    star.kernel_cs_ss = offsetof(struct gdt, kernel_code64);
    star.user_cs_ss = offsetof(struct gdt, user_code32);
    asm_wrmsr(MSR_STAR, star.value);
    asm_wrmsr(MSR_LSTAR, (uint64_t)arch_syscall_stub);
    asm_wrmsr(MSR_SFMASK, RFLAGS_AC | RFLAGS_DF | RFLAGS_IF);

    uint64_t cr0, cr4;
    asm volatile("mov %0, cr0" : "=r"(cr0));
    asm volatile("mov %0, cr4" : "=r"(cr4));

    struct cpuid cpuid1 = asm_cpuid(1, 0);
    struct cpuid cpuid7 = asm_cpuid(7, 0);
    struct cpuid cpuid13 = asm_cpuid(13, 0);

    // Enable SSE.
    cr0 &= ~CR0_EM;
    cr0 |= CR0_MP;
    cr4 |= CR4_OSFXSR | CR4_OSXMMEXCPT;

    if (cpuid1.ecx & CPUID_1C_XSAVE) {
        cr4 |= CR4_OSXSAVE;
        asm volatile("mov cr4, %0" ::"r"(cr4));

        uint64_t xcr0 = XCR0_X87 | XCR0_SSE;
        // AVX
        if (cpuid1.ecx & CPUID_1C_AVX)
            xcr0 |= XCR0_AVX;

        // AVX-512
        if (cpuid7.ebx & CPUID_7B_AVX512F)
            xcr0 |= XCR0_AXV512_OPMASK | XCR0_AVX512_ZMM_HI256 | XCR0_AVX512_HI16_ZMM;

        asm_wrxcr(0, xcr0);

        cpu->arch.fpu_size = cpuid13.ecx;
        cpu->arch.fpu_save = asm_xsave;
        cpu->arch.fpu_restore = asm_xrstor;
    } else {
        cpu->arch.fpu_size = 512;
        cpu->arch.fpu_save = asm_fxsave;
        cpu->arch.fpu_restore = asm_fxrstor;
    }

    if (cpuid7.ecx & CPUID_7C_UMIP)
        cr4 |= CR4_UMIP;

    if (cpuid7.ebx & CPUID_7B_SMEP)
        cr4 |= CR4_SMEP;

    if (cpuid7.ebx & CPUID_7B_SMAP) {
        cr4 |= CR4_SMAP;
        cpu->arch.can_smap = true;
    }

    if (cpuid7.ebx & CPUID_7B_FSGSBASE)
        cr4 |= CR4_FSGSBASE;

    asm volatile("mov %0, cr0" : "=r"(cr0));
    asm volatile("mov %0, cr4" : "=r"(cr4));

    lapic_init(&cpu->arch.lapic);
    cpu->online = true;
}

void percpu_init() {
    hpet_init();
    tsc_init();

    // Init the BSP.
    setup_cpu(&percpu_bsp);

    // Init any APs.
}

struct percpu* percpu_get() {
    struct percpu* result;
    asm volatile("mov %0, gs:0" : "=r"(result)::"memory");
    return result;
}
