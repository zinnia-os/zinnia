// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "mem: " fmt

#include <lib/assert.h>
#include <lib/console.h>
#include <lib/init.h>
#include <lib/initgraph.h>
#include <lib/panic.h>
#include <lib/print.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <mem/pmap.h>
#include <mem/vma.h>

extern uint8_t __ld_text_start[];
extern uint8_t __ld_text_end[];
extern uint8_t __ld_rodata_start[];
extern uint8_t __ld_rodata_end[];
extern uint8_t __ld_data_start[];
extern uint8_t __ld_data_end[];
extern uint8_t __ld_kernel_start[];

struct vma kernel_vma = {0};

// This function creates a kernel page table and initializes all memory managers.
[[__init]]
void mem_bootstrap(struct boot_info* info) {
    struct phys_mem* map = info->mem_map;
    size_t map_len = info->num_mem_maps;
    uintptr_t kernel_virt = info->virt_base;
    phys_t kernel_phys = info->phys_base;
    uintptr_t tmp_hhdm = info->hhdm_base;

    const size_t pgsz = arch_mem_page_size();

    log("Memory map:\n");
    for (size_t i = 0; i < map_len; i++) {
        if (map[i].length == 0)
            continue;

        const char* label = nullptr;
        switch (map[i].usage) {
        case PHYS_RESERVED:
            label = "Reserved";
            break;
        case PHYS_USABLE:
            label = "Usable";
            break;
        case PHYS_STATIC:
            label = "Static";
            break;
        case PHYS_RECLAIMABLE:
            label = "Reclaimable";
            break;
        }

        log("[%p - %p] %s\n", (void*)map[i].address, (void*)(map[i].address + map[i].length - 1), label);
    }

    // Set the HHDM base address to the address given by the loader.
    // We must not keep any virtual addresses to this region,
    // since we're likely going to map it at a different base address.
    mem_hhdm_base = tmp_hhdm;

    assert(pmap_new_kernel(&kernel_vma.pmap, 0) == 0, "Unable to allocate the kernel page table");
    tailq_init(&kernel_vma.mappings);

    // text
    log("Mapping text segment at %p\n", __ld_text_start);
    for (uint8_t* p = __ld_text_start; p < __ld_text_end; p += pgsz) {
        errno_t status = pmap_map(
            &kernel_vma.pmap,
            (uintptr_t)p,
            (phys_t)(p - __ld_kernel_start + kernel_phys),
            PTE_READ | PTE_EXEC,
            CACHE_ANY
        );
        assert(!status, "Failed to map %p with error %i", p, status);
    }

    // rodata
    log("Mapping rodata segment at %p\n", __ld_rodata_start);
    for (uint8_t* p = __ld_rodata_start; p < __ld_rodata_end; p += pgsz) {
        errno_t status = pmap_map(
            &kernel_vma.pmap,
            (uintptr_t)p,
            (phys_t)(p - __ld_kernel_start + kernel_phys),
            PTE_READ,
            CACHE_ANY
        );
        assert(!status, "Failed to map %p with error %i", p, status);
    }

    // data
    log("Mapping data segment at %p\n", __ld_data_start);
    for (uint8_t* p = __ld_data_start; p < __ld_data_end; p += pgsz) {
        errno_t status = pmap_map(
            &kernel_vma.pmap,
            (uintptr_t)p,
            (phys_t)(p - __ld_kernel_start + kernel_phys),
            PTE_READ | PTE_WRITE,
            CACHE_ANY
        );
        assert(!status, "Failed to map %p with error %i", p, status);
    }

    // The kernel address space is divided into 4 segments.
    // For example, they could live at:
    // HHDM     FFFF'8000'0000'0000
    // PFNDB    FFFF'A000'0000'0000
    // Mappings FFFF'C000'0000'0000
    // Kernel   FFFF'FFFF'8000'0000

    // Map all physical memory to the HHDM address.
    tmp_hhdm = arch_mem_hhdm_addr();
    for (size_t i = 0; i < map_len; i++) {
        if (map[i].usage != PHYS_USABLE && map[i].usage != PHYS_STATIC && map[i].usage != PHYS_RECLAIMABLE)
            continue;
        for (size_t p = 0; p < map[i].length; p += pgsz) {
            uintptr_t vaddr = (uintptr_t)(map[i].address + p + tmp_hhdm);
            phys_t paddr = (phys_t)(map[i].address + p);
            errno_t status = pmap_map(&kernel_vma.pmap, vaddr, paddr, PTE_READ | PTE_WRITE, CACHE_ANY);
            assert(!status, "Failed to map HHDM page %p to %p with error %i", (void*)vaddr, (void*)paddr, status);
        }
    }
    mem_hhdm_base = tmp_hhdm;

    // Switch to our own page table.
    pmap_set(&kernel_vma.pmap);

    // We record metadata for every single page of available memory in a large array.
    // This array is contiguous in virtual memory, but is sparsely populated.
    // Only those array entries which represent usable memory are mapped.
    size_t pfndb_pages = 0;
    for (size_t i = 0; i < map_len; i++) {
        if (map[i].length == 0 || map[i].usage != PHYS_USABLE)
            continue;

        const size_t num_pages = (map[i].length + pgsz - 1) / pgsz;
        pfndb_pages = MAX(pfndb_pages, map[i].address / pgsz + num_pages);
        const size_t metadata_size = num_pages * sizeof(struct page);
        const uintptr_t metadata_start_vaddr = arch_mem_pfndb_addr() + (map[i].address / pgsz * sizeof(struct page));
        const uintptr_t metadata_end_vaddr = metadata_start_vaddr + metadata_size;

        // Find the page-aligned boundaries that contain this metadata.
        const uintptr_t aligned_start = ROUND_DOWN(metadata_start_vaddr, pgsz);
        const uintptr_t aligned_end = ROUND_UP(metadata_end_vaddr, pgsz);
        const size_t length = aligned_end - aligned_start;
        const uintptr_t vaddr = aligned_start;

        for (size_t page = 0; page < length; page += pgsz) {
            phys_t paddr;
            assert(!mem_phys_alloc(1, 0, &paddr), "Failed to allocate memory for PFNDB!");
            assert(
                !pmap_map(&kernel_vma.pmap, vaddr + page, paddr, PTE_READ | PTE_WRITE, CACHE_ANY),
                "Failed to map PFNDB memory"
            );
        }
    }
    vm_pfndb = (struct page*)arch_mem_pfndb_addr();
    vm_pfndb_end = vm_pfndb + pfndb_pages;
}
INIT_TASK(mem_init_stage, "mem.bootstrap", mem_bootstrap);
INIT_DEPENDS(mem_init_stage, console_stage);
INIT_ENTAILS(mem_init_stage, mem_stage);

INIT_TASK(mem_stage, "mem.init");
