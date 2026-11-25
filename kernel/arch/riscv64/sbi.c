// SPDX-License-Identifier: BSD-3-Clause

#include <arch/riscv64/sbi.h>
#include <lib/console.h>
#include <lib/init.h>
#include <lib/initgraph.h>

uint64_t sbi_call(uint64_t eid, uint64_t fid, uint64_t a0) {
    register uint64_t r_a0 asm("a0") = a0;
    register uint64_t r_a6 asm("a6") = fid;
    register uint64_t r_a7 asm("a7") = eid;
    asm volatile("ecall" : "+r"(r_a0) : "r"(r_a6), "r"(r_a7) : "a1", "memory");
    return r_a0;
}

static void sbi_write(struct console* con, const char* buf, size_t count) {
    for (size_t i = 0; i < count; i++) {
        sbi_call(SBI_EXT_DBCN, 2, buf[i]);
    }
}

static struct console sbi_console = {
    .name = "sbi",
    .priority = CONSOLE_PRIO_SERIAL,
    .write = sbi_write,
};

[[__init]]
static void sbi_init(struct boot_info*) {
    console_register(&sbi_console);
}
INIT_TASK(sbi_stage, "arch.riscv64.sbi", sbi_init);
INIT_ENTAILS(sbi_stage, console_stage);
