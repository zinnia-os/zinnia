#include <common/utils.h>
#include <kernel/alloc.h>
#include <kernel/assert.h>
#include <kernel/init.h>
#include <kernel/panic.h>
#include <kernel/print.h>
#include <kernel/vmspace.h>
#include <kernel/virt.h>
#include <string.h>

extern uint8_t __ld_text_start[];
extern uint8_t __ld_text_end[];
extern uint8_t __ld_rodata_start[];
extern uint8_t __ld_rodata_end[];
extern uint8_t __ld_data_start[];
extern uint8_t __ld_data_end[];
extern uint8_t __ld_kernel_start[];

struct vmspace kernel_vas = {0};

[[__init]]
void mem_init(struct phys_mem* map, size_t map_len, uintptr_t kernel_virt, phys_t kernel_phys, uintptr_t tmp_hhdm) {
    // This function creates a kernel page table and initializes all memory managers.

    const size_t pgsz = arch_mem_page_size();

    kprintf("Memory map:\n");
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

        kprintf("[%p - %p] %s\n", (void*)map[i].address, (void*)(map[i].address + map[i].length - 1), label);
    }

    // Set up the bootstrap allocator.
    struct phys_mem* largest = map;
    for (size_t i = 0; i < map_len; i++) {
        if (map[i].length > largest->length && map[i].usage == PHYS_USABLE)
            largest = &map[i];
    }
    mem_phys_bootstrap(largest);
    kprintf(
        "Using region [%p - %p] for bootstrap allocator\n",
        (void*)largest->address,
        (void*)(largest->address + largest->length - 1)
    );

    // Set the HHDM base address to the address given by the loader.
    // We must not keep any virtual addresses to this region,
    // since we're likely going to map it at a different base address.
    mem_hhdm_base = tmp_hhdm;

    ASSERT(pmap_new_kernel(&kernel_vas.pmap, 0) == 0, "Unable to allocate the kernel page table\n");

    // text
    kprintf("Mapping text segment at %p\n", __ld_text_start);
    for (uint8_t* p = __ld_text_start; p <= __ld_text_end; p += pgsz) {
        zn_status_t status = pmap_map(
            &kernel_vas.pmap,
            (uintptr_t)p,
            (phys_t)(p - __ld_kernel_start + kernel_phys),
            PTE_READ | PTE_EXEC,
            CACHE_NONE
        );
        ASSERT(!status, "Failed to map %p with error %i\n", p, status);
    }

    // rodata
    kprintf("Mapping rodata segment at %p\n", __ld_rodata_start);
    for (uint8_t* p = __ld_rodata_start; p < __ld_rodata_end; p += pgsz) {
        zn_status_t status =
            pmap_map(&kernel_vas.pmap, (uintptr_t)p, (phys_t)(p - __ld_kernel_start + kernel_phys), PTE_READ, CACHE_NONE);
        ASSERT(!status, "Failed to map %p with error %i\n", p, status);
    }

    // data
    kprintf("Mapping data segment at %p\n", __ld_data_start);
    for (uint8_t* p = __ld_data_start; p < __ld_data_end; p += pgsz) {
        zn_status_t status = pmap_map(
            &kernel_vas.pmap,
            (uintptr_t)p,
            (phys_t)(p - __ld_kernel_start + kernel_phys),
            PTE_READ | PTE_WRITE,
            CACHE_NONE
        );
        ASSERT(!status, "Failed to map %p with error %i\n", p, status);
    }

    // The kernel address space is divided into 3 segments (plus kernel).
    // On x86_64 for example, they live at:
    // HHDM     FFFF'8000'0000'0000
    // PFNDB    FFFF'A000'0000'0000
    // Mappings FFFF'C000'0000'0000

    // Map all physical memory to the HHDM address.
    tmp_hhdm = arch_mem_hhdm_addr();
    for (size_t i = 0; i < map_len; i++) {
        if (map[i].usage != PHYS_USABLE && map[i].usage != PHYS_STATIC)
            continue;
        for (size_t p = 0; p <= map[i].length; p += pgsz) {
            uintptr_t vaddr = (uintptr_t)(map[i].address + p + tmp_hhdm);
            phys_t paddr = (phys_t)(map[i].address + p);
            zn_status_t status = pmap_map(&kernel_vas.pmap, vaddr, paddr, PTE_READ | PTE_WRITE, CACHE_NONE);
            ASSERT(!status, "Failed to map HHDM page %p to %p with error %i\n", (void*)vaddr, (void*)paddr, status);
        }
    }
    mem_hhdm_base = tmp_hhdm;

    // Switch to our own page table.
    pmap_set(&kernel_vas.pmap);

    // We record metadata for every single page of available memory in a large array.
    // This array is contiguous in virtual memory, but is sparsely populated.
    // Only those array entries which represent usable memory are mapped.
    for (size_t i = 0; i < map_len; i++) {
        if (map[i].length == 0 || map[i].usage != PHYS_USABLE)
            continue;

        const size_t num_pages = (map[i].length + pgsz - 1) / pgsz;
        const size_t metadata_size = num_pages * sizeof(struct page);
        const uintptr_t metadata_start_vaddr = arch_vm_pfndb_addr() + (map[i].address / pgsz * sizeof(struct page));
        const uintptr_t metadata_end_vaddr = metadata_start_vaddr + metadata_size;

        // Find the page-aligned boundaries that contain this metadata.
        const uintptr_t aligned_start = ALIGN_DOWN(metadata_start_vaddr, pgsz);
        const uintptr_t aligned_end = ALIGN_UP(metadata_end_vaddr, pgsz);
        const size_t length = aligned_end - aligned_start;
        const uintptr_t vaddr = aligned_start;

        for (size_t page = 0; page < length; page += pgsz) {
            phys_t paddr;
            ASSERT(!mem_phys_alloc(1, 0, &paddr), "Failed to allocate memory for PFNDB!\n");
            pmap_map(&kernel_vas.pmap, vaddr + page, paddr, PTE_READ | PTE_WRITE, CACHE_NONE);
        }
    }
    vm_pfndb = (struct page*)arch_vm_pfndb_addr();

    // We don't need the bootstrap allocator from this point on.
    // Initialize the real page allocator.
    mem_phys_init(map, map_len);

    slab_init();

    kprintf("Memory initialization complete\n");
}
