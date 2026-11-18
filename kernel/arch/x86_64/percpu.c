// SPDX-License-Identifier: BSD-3-Clause

#include <arch/x86_64/apic.h>
#include <arch/x86_64/asm.h>
#include <arch/x86_64/defs.h>
#include <arch/x86_64/gdt.h>
#include <arch/x86_64/idt.h>
#include <arch/x86_64/percpu.h>
#include <arch/x86_64/smp.h>
#include <arch/x86_64/syscall.h>
#include <arch/x86_64/tsc.h>
#include <lib/initgraph.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <config.h>
#include <stddef.h>

void arch_percpu_bsp_init() {
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

static_assert(sizeof(union ia32_star) == 8);

[[__percpu]]
struct arch_percpu arch_data = {0};

void x86_64_cpu_setup(struct percpu* cpu) {
    asm_wrmsr(MSR_KERNEL_GS_BASE, 0);
    asm_wrmsr(MSR_GS_BASE, (uint64_t)cpu);
    asm_wrmsr(MSR_FS_BASE, 0);

    struct arch_percpu* arch_cpu = percpu_ref(arch_data);

    gdt_load();

    // TODO: not if FRED
    idt_load();

    asm_wrmsr(
        MSR_PAT,
        PAT_WB | PAT_WT << 8 | PAT_UCM << 16 | PAT_UC << 24 | (uint64_t)PAT_WP << 32 | (uint64_t)PAT_WC << 40 |
            (uint64_t)PAT_UCM << 48 | (uint64_t)PAT_UC << 56
    );

    // Syscall extension.
    asm_wrmsr(MSR_EFER, asm_rdmsr(MSR_EFER) | MSR_EFER_SCE);
    union ia32_star star = {0};
    star.kernel_cs_ss = offsetof(struct gdt, kernel_code64);
    star.user_cs_ss = offsetof(struct gdt, user_code32) | CPL_USER;
    asm_wrmsr(MSR_STAR, star.value);
    asm_wrmsr(MSR_LSTAR, (uint64_t)arch_syscall_stub);
    // Mask all RFLAGS except for BRKI which is reserved.
    asm_wrmsr(MSR_SFMASK, ~RFLAGS_BRKI);

    // TODO: FRED

    uint64_t cr0, cr4;
    asm volatile("mov %0, cr0" : "=r"(cr0));
    asm volatile("mov %0, cr4" : "=r"(cr4));

    struct cpuid cpuid1 = asm_cpuid(1, 0);
    struct cpuid cpuid7 = asm_cpuid(7, 0);
    struct cpuid cpuid13 = asm_cpuid(13, 0);

    // Enable SSE.
    cr0 &= ~CR0_EM;
    cr0 |= CR0_MP | CR0_WP;
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

        arch_cpu->fpu_size = cpuid13.ecx;
        arch_cpu->fpu_save = asm_xsave;
        arch_cpu->fpu_restore = asm_xrstor;
    } else {
        arch_cpu->fpu_size = 512;
        arch_cpu->fpu_save = asm_fxsave;
        arch_cpu->fpu_restore = asm_fxrstor;
    }

    if (cpuid7.ecx & CPUID_7C_UMIP)
        cr4 |= CR4_UMIP;

    if (cpuid7.ebx & CPUID_7B_SMEP)
        cr4 |= CR4_SMEP;

    if (cpuid7.ebx & CPUID_7B_SMAP) {
        cr4 |= CR4_SMAP;
        arch_cpu->can_smap = true;
    }

    if (cpuid7.ebx & CPUID_7B_FSGSBASE)
        cr4 |= CR4_FSGSBASE;

    asm volatile("mov cr0, %0" ::"r"(cr0));
    asm volatile("mov cr4, %0" ::"r"(cr4));

    lapic_init(&arch_cpu->lapic);
    cpu->online = true;
}

void arch_percpu_init() {
#if CONFIG_TSC
    tsc_init();
#endif

    // Init the BSP.
    x86_64_cpu_setup(&percpu_bsp);

    // Init any APs.
    x86_64_smp_init();
}
