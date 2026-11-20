// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "smp: " fmt

#include <arch/x86_64/apic.h>
#include <arch/x86_64/defs.h>
#include <arch/x86_64/percpu.h>
#include <arch/x86_64/pmap.h>
#include <arch/x86_64/smp.h>
#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/clock.h>
#include <lib/compiler.h>
#include <lib/irq.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/sched.h>
#include <lib/types.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <uacpi/acpi.h>
#include <uacpi/tables.h>

extern uint8_t smp_trampoline_start[];
extern uint8_t smp_trampoline_data[];
extern uint8_t smp_trampoline_end[];

DEFINE_ASM(
    ".text.smp_trampoline",
    "ax",
    ".code16\n"
    ".global smp_trampoline_start\n"
    "smp_trampoline_start:\n"
    "    cli\n"
    "    cld\n"
    "    jmp .Lmode16\n"

    ".org smp_trampoline_start + %c[data_off]\n"
    ".global smp_trampoline_data\n"
    "smp_trampoline_data:\n"
    "    .skip %c[info_size]\n"

    ".set .Ldata_offset, %c[data_off]\n"
    ".set .Lgdtr_offset, (.Ldata_offset + %c[gdtr])\n"
    ".set .Lfarjmp_offset, (.Ldata_offset + %c[farjmp])\n"
    ".set .Ltemp_stack_offset, (.Ldata_offset + %c[temp_stack])\n"
    ".set .Ltemp_cr3_offset, (.Ldata_offset + %c[temp_cr3])\n"
    ".set .Lentry_offset, (.Ldata_offset + %c[entry])\n"
    ".set .Lhhdm_offset, (.Ldata_offset + %c[hhdm])\n"
    ".set .Linvalid_idtr_offset, (.Linvalid_idtr - smp_trampoline_start)\n"
    ".set .Lmode32_offset, (.Lmode32 - smp_trampoline_start)\n"
    ".set .Lmode64_offset, (.Lmode64 - smp_trampoline_start)\n"

    ".Lmode16:\n"
    "    mov bx, cs\n"
    "    shl ebx, 4\n"

    "    lidtd cs:.Linvalid_idtr_offset\n"
    "    lgdtd cs:.Lgdtr_offset\n"

    "    lea eax, [ebx + .Lmode32_offset]\n"
    "    mov dword ptr cs:.Lfarjmp_offset, eax\n"

    "    mov eax, %c[cr0]\n"
    "    mov cr0, eax\n"
    "    jmp fword ptr cs:.Lfarjmp_offset\n"

    ".code32\n"
    ".Lmode32:\n"
    "    mov ax, %c[kernel32_ds]\n"
    "    mov ds, ax\n"
    "    mov es, ax\n"
    "    mov fs, ax\n"
    "    mov gs, ax\n"
    "    mov ss, ax\n"

    "    xor eax, eax\n"
    "    lldt ax\n"

    "    mov esp, dword ptr [ebx + .Ltemp_stack_offset]\n"

    "    xor eax, eax\n"
    "    mov cr4, eax\n"

    "    or eax, %c[cr4_pae]\n"
    "    mov cr4, eax\n"

    "    mov ecx, %c[msr_efer]\n"
    "    mov eax, %c[efer_lme]\n"
    "    xor edx, edx\n"
    "    wrmsr\n"

    "    mov eax, dword ptr [ebx + .Ltemp_cr3_offset]\n"
    "    mov cr3, eax\n"

    "    mov eax, cr0\n"
    "    or eax, %c[cr0_pg]\n"
    "    mov cr0, eax\n"

    "    lea eax, [ebx + .Lmode64_offset]\n"
    "    push %c[kernel64_cs]\n"
    "    push eax\n"
    "    retf\n"

    ".code64\n"
    ".Lmode64:\n"
    "    mov ax, %c[kernel64_ds]\n"
    "    mov ds, ax\n"
    "    mov es, ax\n"
    "    mov fs, ax\n"
    "    mov gs, ax\n"
    "    mov ss, ax\n"

    "    mov ecx, %c[msr_efer]\n"
    "    rdmsr\n"
    "    or eax, %c[efer_nxe]\n"
    "    wrmsr\n"

    "    xor ebp, ebp\n"

    "    lea rdi, [rbx + .Ldata_offset]\n"
    "    mov rax, [rbx + .Lentry_offset]\n"
    "    add rsp, [rbx + .Lhhdm_offset]\n"
    "    jmp rax\n"

    ".Linvalid_idtr:\n"
    "    .word 0\n"
    "    .quad 0\n"

    ".global smp_trampoline_end\n"
    "smp_trampoline_end:\n",
    [data_off] "i"(8),
    [info_size] "i"(sizeof(struct smp_info)),
    [gdtr] "i"(offsetof(struct smp_info, gdtr)),
    [farjmp] "i"(offsetof(struct smp_info, farjmp_offset)),
    [temp_stack] "i"(offsetof(struct smp_info, temp_stack)),
    [temp_cr3] "i"(offsetof(struct smp_info, temp_cr3)),
    [entry] "i"(offsetof(struct smp_info, entry)),
    [hhdm] "i"(offsetof(struct smp_info, hhdm_offset)),
    [kernel32_ds] "i"(offsetof(struct gdt, kernel_data32)),
    [kernel64_cs] "i"(offsetof(struct gdt, kernel_code64)),
    [kernel64_ds] "i"(offsetof(struct gdt, kernel_data64)),
    [cr0] "i"(CR0_PE | CR0_ET),
    [cr0_pg] "i"((uint32_t)CR0_PG),
    [cr4_pae] "i"(CR4_PAE),
    [msr_efer] "i"(MSR_EFER),
    [efer_lme] "i"(MSR_EFER_LME),
    [efer_nxe] "i"(MSR_EFER_NXE)
);

constexpr size_t AP_BOOT_STACK_SIZE = 0x8000;
constexpr uint64_t AP_BOOT_TIMEOUT_NS = 1'000'000'000;

[[noreturn]]
static void ap_entry(phys_t info_addr) {
    // We're still running on the temporary page table.
    pmap_set(&kernel_vma.pmap);

    struct smp_info* info = HHDM_PTR(info_addr);
    struct percpu* cpu = (struct percpu*)info->percpu;

    x86_64_cpu_setup(cpu);
    irq_lock();

    // This task keeps running on the boot stack, but is never scheduled again after the first yield.
    sched_init(&cpu->sched);

    // Let the BSP know that we're alive.
    // `info` must not be touched after this point.
    atomic_store(&info->booted, 1, ATOMIC_RELEASE);

    log("CPU %zu online\n", cpu->id);

    sched_yield(&cpu->sched);
    unreachable();
}

static bool start_ap(phys_t trampoline, uint32_t temp_cr3, uint32_t lapic_id) {
    const size_t trampoline_size = smp_trampoline_end - smp_trampoline_start;

    phys_t stack;
    if (mem_phys_alloc(AP_BOOT_STACK_SIZE / arch_page_size(), ALLOC_MEM32 | ALLOC_NOZERO, &stack)) {
        log_err("Failed to allocate a boot stack for LAPIC %u\n", lapic_id);
        return false;
    }

    struct percpu* cpu = percpu_new();
    if (!cpu) {
        log_err("Failed to allocate per-CPU data for LAPIC %u\n", lapic_id);
        mem_phys_free(stack, AP_BOOT_STACK_SIZE / arch_page_size());
        return false;
    }

    cpu->boot_stack = (struct stack){
        .base = (uintptr_t)HHDM_PTR(stack),
        .phys = stack,
        .size = AP_BOOT_STACK_SIZE,
    };

    log("Starting CPU %zu\n", cpu->id);

    memcpy(HHDM_PTR(trampoline), smp_trampoline_start, trampoline_size);

    const phys_t info_addr = trampoline + (smp_trampoline_data - smp_trampoline_start);
    struct smp_info* info = HHDM_PTR(info_addr);
    memcpy(&info->gdt, &gdt_base, sizeof(info->gdt));
    info->gdtr.limit = sizeof(struct gdt) - 1;
    info->gdtr.base = (struct gdt*)(info_addr + offsetof(struct smp_info, gdt));
    info->farjmp_offset = 0; // Filled in by the trampoline.
    info->farjmp_segment = offsetof(struct gdt, kernel_code32);
    info->temp_stack = stack + AP_BOOT_STACK_SIZE;
    info->temp_cr3 = temp_cr3;
    info->entry = (uint64_t)ap_entry;
    info->hhdm_offset = mem_hhdm_base;
    info->percpu = (uint64_t)cpu;
    info->lapic_id = lapic_id;
    atomic_init(&info->booted, 0);

    struct local_apic* lapic = &percpu_ref(arch_data)->lapic;

    lapic_send_ipi(lapic, lapic_id, 0, APIC_DELIVERY_INIT, APIC_DEST_PHYSICAL, APIC_LEVEL_ASSERT, APIC_TRIGGER_EDGE);
    clock_spin_ns(10'000'000);

    lapic_send_ipi(
        lapic,
        lapic_id,
        trampoline >> 12,
        APIC_DELIVERY_STARTUP,
        APIC_DEST_PHYSICAL,
        APIC_LEVEL_ASSERT,
        APIC_TRIGGER_EDGE
    );
    clock_spin_ns(10'000'000);

    for (uint64_t waited = 0; !atomic_load(&info->booted, ATOMIC_ACQUIRE); waited += 1'000'000) {
        if (waited >= AP_BOOT_TIMEOUT_NS) {
            // The per-CPU data and boot stack are leaked on purpose, in case the AP still comes up.
            log_err("CPU %zu (LAPIC %u) did not respond\n", cpu->id, lapic_id);
            return false;
        }
        clock_spin_ns(1'000'000);
    }

    return true;
}

static errno_t build_temp_table(phys_t* out) {
    const size_t pgsz = arch_page_size();

    phys_t mem;
    errno_t status = mem_phys_alloc(6, ALLOC_MEM32, &mem);
    if (status)
        return status;

    uint64_t* l4 = HHDM_PTR(mem);
    uint64_t* l3 = HHDM_PTR(mem + pgsz);
    const phys_t l2_base = mem + 2 * pgsz;

    // Use 2 MiB pages.
    l4[0] = (mem + pgsz) | ARCH_FLAG_PRESENT | ARCH_FLAG_READ_WRITE;
    for (size_t i = 0; i < 4; i++) {
        uint64_t* l2 = HHDM_PTR(l2_base + i * pgsz);
        l3[i] = (l2_base + i * pgsz) | ARCH_FLAG_PRESENT | ARCH_FLAG_READ_WRITE;
        for (size_t j = 0; j < 512; j++)
            l2[j] = ((i << 30) + (j << 21)) | ARCH_FLAG_PRESENT | ARCH_FLAG_READ_WRITE | ARCH_FLAG_SIZE;
    }

    // Copy over the higher half from the kernel page table.
    const uint64_t* kernel_l4 = HHDM_PTR(kernel_vma.pmap.root);
    for (size_t i = 256; i < 512; i++)
        l4[i] = kernel_l4[i];

    *out = mem;
    return 0;
}

void x86_64_smp_init() {
    const size_t trampoline_size = smp_trampoline_end - smp_trampoline_start;
    assert(trampoline_size <= arch_page_size(), "SMP trampoline doesn't fit in a page");

    const size_t max_cpus = percpu_max_cpus();
    if (max_cpus <= 1)
        return;

    uacpi_table table;
    if (uacpi_table_find_by_signature(ACPI_MADT_SIGNATURE, &table) != UACPI_STATUS_OK) {
        log_warn("No MADT found, not starting any APs\n");
        return;
    }

    phys_t trampoline;
    if (mem_phys_alloc(1, ALLOC_MEM20, &trampoline)) {
        log_err("Failed to allocate 20-bit memory for the AP trampoline\n");
        uacpi_table_unref(&table);
        return;
    }

    phys_t temp_table;
    if (build_temp_table(&temp_table)) {
        log_err("Failed to allocate a temporary page table\n");
        mem_phys_free(trampoline, 1);
        uacpi_table_unref(&table);
        return;
    }

    const uint32_t bsp_id = percpu_ref(arch_data)->lapic.id;
    bool all_booted = true;
    size_t num_cpus = 1;

    const struct acpi_madt* madt = table.ptr;
    const uint8_t* entry = (const uint8_t*)madt->entries;
    const uint8_t* end = (const uint8_t*)madt + madt->hdr.length;

    while (entry + sizeof(struct acpi_entry_hdr) <= end && num_cpus < max_cpus) {
        const struct acpi_entry_hdr* hdr = (const struct acpi_entry_hdr*)entry;

        // Don't trust malformed firmware tables.
        if (hdr->length < sizeof(struct acpi_entry_hdr) || entry + hdr->length > end) {
            log_warn("MADT contains a malformed entry\n");
            break;
        }

        uint32_t lapic_id = 0;
        bool enabled = false;
        switch (hdr->type) {
        case ACPI_MADT_ENTRY_TYPE_LAPIC:
            const struct acpi_madt_lapic* lapic = (const struct acpi_madt_lapic*)hdr;
            lapic_id = lapic->id;
            enabled = lapic->flags & ACPI_PIC_ENABLED;
            break;
        case ACPI_MADT_ENTRY_TYPE_LOCAL_X2APIC:
            const struct acpi_madt_x2apic* x2apic = (const struct acpi_madt_x2apic*)hdr;
            lapic_id = x2apic->id;
            enabled = x2apic->flags & ACPI_PIC_ENABLED;
            break;
        }

        if (enabled && lapic_id != bsp_id) {
            if (start_ap(trampoline, temp_table, lapic_id))
                num_cpus++;
            else
                all_booted = false;
        }

        entry += hdr->length;
    }

    uacpi_table_unref(&table);

    // An AP which didn't respond might still be using these.
    if (all_booted) {
        mem_phys_free(temp_table, 6);
        mem_phys_free(trampoline, 1);
    }

    log("%zu CPU(s) online\n", num_cpus);
}
