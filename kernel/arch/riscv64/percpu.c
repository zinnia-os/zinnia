// SPDX-License-Identifier: BSD-3-Clause

#include <arch/riscv64/defs.h>
#include <arch/riscv64/sbi.h>
#include <arch/riscv64/trap.h>
#include <lib/percpu.h>
#include <bits/mem.h>
#include <stdint.h>

uint8_t riscv_pmap_levels = 4;

void arch_percpu_bsp_init() {
    uint64_t satp;
    asm volatile("csrr %0, satp" : "=r"(satp));
    riscv_pmap_levels = arch_satp_levels(satp);

    asm volatile("la tp, percpu_bsp" ::: "tp");
    asm volatile("csrw sscratch, zero" ::: "memory");
    asm volatile("csrw stvec, %0" ::"r"(stvec_entry) : "memory");
}

void arch_percpu_init() {
    sbi_set_timer(UINT64_MAX);
    asm volatile("csrs sie, %0" ::"r"(SIE_STIE) : "memory");
}
