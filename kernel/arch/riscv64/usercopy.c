// SPDX-License-Identifier: BSD-3-Clause

#include <arch/riscv64/defs.h>
#include <lib/compiler.h>
#include <mem/usercopy.h>

void arch_usercopy_allow(bool state) {
    if (state)
        asm volatile("csrs sstatus, %0" ::"r"(SSTATUS_SUM) : "memory");
    else
        asm volatile("csrc sstatus, %0" ::"r"(SSTATUS_SUM) : "memory");
}

extern void riscv64_read_start();
extern void riscv64_read_end();
extern void riscv64_read_fault();

static const struct usercopy_region read_region = {
    .start_ip = riscv64_read_start,
    .end_ip = riscv64_read_end,
    .fault_ip = riscv64_read_fault,
};

DEFINE_ASM_FUNC(
    arch_usercopy_read,
    "    lla t0, %c0\n"
    "    sd t0, 0(a3)\n"
    ".global riscv64_read_start\n"
    "riscv64_read_start:\n"
    "    beqz a2, 2f\n"
    "1:\n"
    "    lbu t1, 0(a1)\n"
    "    sb t1, 0(a0)\n"
    "    addi a0, a0, 1\n"
    "    addi a1, a1, 1\n"
    "    addi a2, a2, -1\n"
    "    bnez a2, 1b\n"
    "2:\n"
    ".global riscv64_read_end\n"
    "riscv64_read_end:\n"
    "    sd zero, 0(a3)\n"
    "    li a0, 1\n"
    "    ret\n"
    ".global riscv64_read_fault\n"
    "riscv64_read_fault:\n"
    "    sd zero, 0(a3)\n"
    "    li a0, 0\n"
    "    ret\n",
    "i"(&read_region)
);

extern void riscv64_write_start();
extern void riscv64_write_end();
extern void riscv64_write_fault();

static const struct usercopy_region write_region = {
    .start_ip = riscv64_write_start,
    .end_ip = riscv64_write_end,
    .fault_ip = riscv64_write_fault,
};

DEFINE_ASM_FUNC(
    arch_usercopy_write,
    "    lla t0, %c0\n"
    "    sd t0, 0(a3)\n"
    ".global riscv64_write_start\n"
    "riscv64_write_start:\n"
    "    beqz a2, 2f\n"
    "1:\n"
    "    lbu t1, 0(a1)\n"
    "    sb t1, 0(a0)\n"
    "    addi a0, a0, 1\n"
    "    addi a1, a1, 1\n"
    "    addi a2, a2, -1\n"
    "    bnez a2, 1b\n"
    "2:\n"
    ".global riscv64_write_end\n"
    "riscv64_write_end:\n"
    "    sd zero, 0(a3)\n"
    "    li a0, 1\n"
    "    ret\n"
    ".global riscv64_write_fault\n"
    "riscv64_write_fault:\n"
    "    sd zero, 0(a3)\n"
    "    li a0, 0\n"
    "    ret\n",
    "i"(&write_region)
);

extern void riscv64_strlen_start();
extern void riscv64_strlen_end();
extern void riscv64_strlen_fault();

static const struct usercopy_region strlen_region = {
    .start_ip = riscv64_strlen_start,
    .end_ip = riscv64_strlen_end,
    .fault_ip = riscv64_strlen_fault,
};

DEFINE_ASM_FUNC(
    arch_usercopy_strlen,
    "    lla t0, %c0\n"
    "    sd t0, 0(a3)\n"
    ".global riscv64_strlen_start\n"
    "riscv64_strlen_start:\n"
    "    li t1, 0\n"
    "1:\n"
    "    beq t1, a1, 2f\n"
    "    add t2, a0, t1\n"
    "    lbu t3, 0(t2)\n"
    "    beqz t3, 2f\n"
    "    addi t1, t1, 1\n"
    "    j 1b\n"
    "2:\n"
    "    sd t1, 0(a2)\n"
    ".global riscv64_strlen_end\n"
    "riscv64_strlen_end:\n"
    "    sd zero, 0(a3)\n"
    "    li a0, 1\n"
    "    ret\n"
    ".global riscv64_strlen_fault\n"
    "riscv64_strlen_fault:\n"
    "    sd zero, 0(a3)\n"
    "    li a0, 0\n"
    "    ret\n",
    "i"(&strlen_region)
);
