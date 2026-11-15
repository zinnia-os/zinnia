// SPDX-License-Identifier: BSD-3-Clause

#include <arch/x86_64/apic.h>
#include <arch/x86_64/defs.h>
#include <arch/x86_64/percpu.h>
#include <arch/x86_64/pmap.h>
#include <lib/irq.h>
#include <lib/percpu.h>
#include <lib/types.h>
#include <mem/pmap.h>

void pte_clear(pte_t* pte) {
    *pte = 0;
}

void pte_write(pte_t* pte, pte_t val) {
    *pte = val;
}

void pte_flush(uintptr_t vaddr) {
    asm volatile("invlpg [%0]" ::"r"(vaddr) : "memory");
}

void pte_flush_all() {
    uint64_t cr3;
    asm volatile("mov %0, cr3" : "=r"(cr3));
    asm volatile("mov cr3, %0" ::"r"(cr3) : "memory");
}

void pte_shootdown_kick(struct percpu* cpu) {
    irq_lock();
    lapic_send_ipi(
        &percpu_ref(arch_data)->lapic,
        percpu_ref_on(cpu, arch_data)->lapic.id,
        IDT_IPI_SHOOTDOWN,
        APIC_DELIVERY_FIXED,
        APIC_DEST_PHYSICAL,
        APIC_LEVEL_ASSERT,
        APIC_TRIGGER_EDGE
    );
    irq_unlock();
}

pte_t pte_build(phys_t addr, enum pte_flags flags, enum cache_mode cache) {
    pte_t result = ((pte_t)addr & ARCH_PTE_ADDR_MASK) | ARCH_FLAG_PRESENT;

    if (flags & PTE_USER)
        result |= ARCH_FLAG_USER_MODE;

    if (flags & PTE_DIR)
        result |= ARCH_FLAG_READ_WRITE;
    else {
        if (flags & PTE_WRITE)
            result |= ARCH_FLAG_READ_WRITE;
        if (!(flags & PTE_EXEC))
            result |= ARCH_FLAG_EXECUTE_DISABLE;

        switch (cache) {
        case CACHE_ANY:
        case CACHE_WRITE_BACK:
            break;
        case CACHE_WRITE_THROUGH:
            result |= ARCH_FLAG_WRITE_THROUGH;
            break;
        case CACHE_MMIO:
            result |= ARCH_FLAG_CACHE_DISABLE | ARCH_FLAG_WRITE_THROUGH;
            break;
        case CACHE_WRITE_COMBINE:
            result |= ARCH_FLAG_ATTRIBUTE_TABLE | ARCH_FLAG_WRITE_THROUGH;
            break;
        }
    }
    return result;
}

pte_t pte_protect(pte_t old, enum pte_flags flags) {
    pte_t result = old & ~(ARCH_FLAG_READ_WRITE | ARCH_FLAG_USER_MODE | ARCH_FLAG_EXECUTE_DISABLE);
    if (flags & PTE_USER)
        result |= ARCH_FLAG_USER_MODE;
    if (flags & PTE_WRITE)
        result |= ARCH_FLAG_READ_WRITE;
    if (!(flags & PTE_EXEC))
        result |= ARCH_FLAG_EXECUTE_DISABLE;
    return result;
}

bool pte_is_present(pte_t* pte) {
    return *pte & ARCH_FLAG_PRESENT;
}

phys_t pte_address(pte_t* pte) {
    return (phys_t)(*pte & ARCH_PTE_ADDR_MASK);
}

void pmap_set(struct pmap* pt) {
    asm volatile("mov cr3, %0" ::"r"(pt->root) : "memory");
}
