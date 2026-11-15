// SPDX-License-Identifier: BSD-3-Clause

#include <arch/x86_64/asm.h>
#include <lib/initgraph.h>
#include <lib/print.h>
#include <lib/random.h>

constexpr uint32_t CPUID_1C_RDRAND = 1u << 30;
constexpr int RDRAND_RETRIES = 10;

static bool rdrand_get_u64(struct random_source*, uint64_t* out) {
    for (int i = 0; i < RDRAND_RETRIES; i++) {
        uint64_t value;
        uint8_t ok;
        asm volatile("rdrand %0\n setc %1" : "=r"(value), "=qm"(ok) : : "cc");
        if (ok) {
            *out = value;
            return true;
        }
    }
    return false;
}

static struct random_source rdrand_source = {
    .name = "x86_64.rdrand",
    .quality = 100,
    .get_u64 = rdrand_get_u64,
};

static void rdrand_init(struct boot_info*) {
    if (asm_cpuid(1, 0).ecx & CPUID_1C_RDRAND) {
        random_register(&rdrand_source);
    } else {
        log_warn("random: This CPU doesn't support RDRAND, ignoring\n");
    }
}
INIT_TASK(rdrand_stage, "arch.x86_64.rdrand", rdrand_init);
INIT_ENTAILS(rdrand_stage, random_stage);
