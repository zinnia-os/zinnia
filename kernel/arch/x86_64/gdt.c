#include <kernel/percpu.h>
#include <kernel/syscall.h>
#include <stddef.h>
#include <string.h>
#include <x86_64/asm.h>
#include <x86_64/defs.h>
#include <x86_64/gdt.h>
#include <x86_64/tss.h>

static const struct gdt main_gdt = {
    .null = {.val = 0},
    .kernel_code32 = {.val = 0x00cf9b000000ffff},
    .kernel_data32 = {.val = 0x00cf93000000ffff},
    .kernel_code64 = {.val = 0x00a09b0000000000},
    .kernel_data64 = {.val = 0x0000930000000000},
    .user_code32 = {.val = 0x00cffb000000ffff},
    .user_data = {.val = 0x0000f30000000000},
    .user_code64 = {.val = 0x00a0fb0000000000},
    .tss = {
        .limit0 = sizeof(struct tss) - 1,
        .access = 0x89, /* Present | DPL=0 | System | 64-bit TSS */
        .limit1_flags = ((sizeof(struct tss) - 1) >> 16) & 0x0F,
        .reserved = 0,
    },
};

void gdt_load() {
    struct gdt* gdt = &percpu_get()->arch.gdt;
    memcpy(gdt, &main_gdt, sizeof(main_gdt));

    struct tss* tss = &percpu_get()->arch.tss;
    tss->iopb = sizeof(struct tss);

    uintptr_t tss_addr = (uintptr_t)tss;
    gdt->tss.base0 = tss_addr;
    gdt->tss.base1 = tss_addr >> 16;
    gdt->tss.base2 = tss_addr >> 24;
    gdt->tss.base3 = tss_addr >> 32;

    struct gdtr gdtr = {
        .limit = sizeof(struct gdt) - 1,
        .base = gdt,
    };

    asm volatile("lgdt %0" ::"m"(gdtr));

    // Save the contents of MSR_GS_BASE, as they get cleared by a write to `gs`.
    uintptr_t gs = asm_rdmsr(MSR_GS_BASE);

    // Flush and reload the segment registers.
    asm volatile(
        "push %0\n"
        "lea rax, [rip + 1f]\n"
        "push rax\n"
        "retfq\n"
        "1:\n"
        "mov ax, %1\n"
        "mov ds, ax\n"
        "mov es, ax\n"
        "mov fs, ax\n"
        "mov gs, ax\n"
        "mov ss, ax\n"
        :
        : "i"(offsetof(struct gdt, kernel_code64)), "i"(offsetof(struct gdt, kernel_data64))
        : "rax", "memory"
    );

    asm_wrmsr(MSR_GS_BASE, gs);

    asm volatile(
        "mov ax, %c0\n"
        "ltr ax"
        :
        : "i"(offsetof(struct gdt, tss))
        : "rax", "memory"
    );
}
