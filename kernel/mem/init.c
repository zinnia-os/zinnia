#include <kernel/assert.h>
#include <kernel/common.h>
#include <kernel/mem.h>
#include <kernel/panic.h>
#include <kernel/print.h>
#include <string.h>

extern uint8_t __ld_text_start[];
extern uint8_t __ld_text_end[];
extern uint8_t __ld_rodata_start[];
extern uint8_t __ld_rodata_end[];
extern uint8_t __ld_data_start[];
extern uint8_t __ld_data_end[];
extern uint8_t __ld_kernel_start[];

struct page_table mem_kernel_table = {0};

void mem_init(struct phys_mem* map, size_t map_len, virt_t kernel_virt, phys_t kernel_phys, virt_t tmp_hhdm) {
    // This function creates a kernel page table and initializes all memory managers.

    const size_t pgsz = mem_page_size();

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

    ASSERT(mem_pt_new_kernel(&mem_kernel_table, 0) == 0, "Unable to allocate the kernel page table");

    // text
    kprintf("Mapping text segment at %p\n", __ld_text_start);
    for (uint8_t* p = __ld_text_start; p <= __ld_text_end; p += pgsz) {
        menix_status_t status = mem_pt_map(
            &mem_kernel_table,
            (virt_t)p,
            (phys_t)(p - __ld_kernel_start + kernel_phys),
            PTE_READ | PTE_EXEC,
            CACHE_NONE
        );
        ASSERT(!status, "Failed to map %p with error %i", p, status);
    }

    // rodata
    kprintf("Mapping rodata segment at %p\n", __ld_rodata_start);
    for (uint8_t* p = __ld_rodata_start; p < __ld_rodata_end; p += pgsz) {
        menix_status_t status = mem_pt_map(
            &mem_kernel_table,
            (virt_t)p,
            (phys_t)(p - __ld_kernel_start + kernel_phys),
            PTE_READ,
            CACHE_NONE
        );
        ASSERT(!status, "Failed to map %p with error %i", p, status);
    }

    // data
    kprintf("Mapping data segment at %p\n", __ld_data_start);
    for (uint8_t* p = __ld_data_start; p < __ld_data_end; p += pgsz) {
        menix_status_t status = mem_pt_map(
            &mem_kernel_table,
            (virt_t)p,
            (phys_t)(p - __ld_kernel_start + kernel_phys),
            PTE_READ | PTE_WRITE,
            CACHE_NONE
        );
        ASSERT(!status, "Failed to map %p with error %i", p, status);
    }

    // The kernel address space is divided into 3 segments (plus kernel).
    // On x86_64 for example, they live at:
    // HHDM     FFFF'8000'0000'0000
    // PFNDB    FFFF'A000'0000'0000
    // Mappings FFFF'C000'0000'0000

    // Map all physical memory to the HHDM address.
    tmp_hhdm = mem_hhdm_addr();
    for (size_t i = 0; i < map_len; i++) {
        for (size_t p = 0; p <= map[i].length; p += pgsz) {
            virt_t vaddr = (virt_t)(map[i].address + p + tmp_hhdm);
            phys_t paddr = (phys_t)(map[i].address + p);
            menix_status_t status = mem_pt_map(&mem_kernel_table, vaddr, paddr, PTE_READ | PTE_WRITE, CACHE_NONE);
            ASSERT(!status, "Failed to map HHDM page %p to %p with error %i", (void*)vaddr, (void*)paddr, status);
        }
    }
    mem_hhdm_base = tmp_hhdm;

    // Switch to our own page table.
    mem_pt_set(&mem_kernel_table);

    // We record metadata for every single page of available memory in a large array.
    // This array is contiguous in virtual memory, but is sparsely populated.
    // Only those array entries which represent usable memory are mapped.
    for (size_t i = 0; i < map_len; i++) {
        if (map[i].length == 0 || map[i].usage != PHYS_USABLE)
            continue;

        size_t length = ALIGN_UP((map[i].length / pgsz) * sizeof(struct page), pgsz);
        virt_t vaddr = ALIGN_DOWN(mem_pfndb_addr() + (map[i].address / pgsz * sizeof(struct page)), pgsz);

        for (size_t page = 0; page < length; page += pgsz) {
            phys_t paddr;
            ASSERT(!mem_phys_alloc(1, 0, &paddr), "Failed to allocate memory for PFNDB!");
            mem_pt_map(&mem_kernel_table, vaddr + page, paddr, PTE_READ | PTE_WRITE, CACHE_NONE);
        }
    }
    mem_pfndb = (struct page*)mem_pfndb_addr();

    // We don't need the bootstrap allocator from this point on.
    // Initialize the real page allocator.
    mem_phys_init(map, map_len);

    kprintf("Memory initialization complete\n");
}
