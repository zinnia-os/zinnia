// SPDX-License-Identifier: BSD-3-Clause

#include <arch/percpu.h>
#include <lib/atomic.h>
#include <lib/cmdline.h>
#include <lib/compiler.h>
#include <lib/percpu.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/pmap.h>
#include <mem/vma.h>

static _Atomic(size_t) num_cpus = 1;

static size_t max_cpus = SIZE_MAX;
static void smp_option(uintptr_t value) {
    max_cpus = value;
}
CMDLINE_OPTION("smp", "Maximum number of CPUs to use", smp_option);

[[__used, __section(".percpu.init")]]
struct percpu percpu_bsp = {
    .self = &percpu_bsp,
    .id = 0,
    .online = false,
};

struct percpu* percpu_new() {
    const size_t pgsz = arch_mem_page_size();
    const size_t stride = percpu_stride();
    const size_t id = atomic_load(&num_cpus, ATOMIC_RELAXED);
    if (id >= (UINTPTR_MAX - (uintptr_t)__ld_percpu_start) / stride)
        return nullptr;

    phys_t mem;
    if (mem_phys_alloc(stride / pgsz, 0, &mem))
        return nullptr;

    const uintptr_t base = (uintptr_t)percpu_cpu(id);
    for (size_t offset = 0; offset < stride; offset += pgsz) {
        if (pmap_map(&kernel_vma.pmap, base + offset, mem + offset, PTE_READ | PTE_WRITE, CACHE_ANY)) {
            while (offset) {
                offset -= pgsz;
                pmap_unmap(&kernel_vma.pmap, base + offset);
            }
            mem_phys_free(mem, stride / pgsz);
            return nullptr;
        }
    }

    struct percpu* cpu = (struct percpu*)base;
    cpu->self = cpu;
    cpu->id = id;
    cpu->online = false;
    atomic_store(&num_cpus, id + 1, ATOMIC_RELEASE);
    return cpu;
}

size_t percpu_max_cpus() {
    return max_cpus;
}

size_t percpu_count() {
    return atomic_load(&num_cpus, ATOMIC_RELAXED);
}

struct percpu* percpu_get() {
    return arch_percpu_get();
}

void percpu_bsp_init() {
    arch_percpu_bsp_init();
}

void percpu_init(struct boot_info*) {
    arch_percpu_init();
}
INIT_TASK(percpu_stage, "lib.percpu", percpu_init);
INIT_DEPENDS(percpu_stage, mem_stage);
