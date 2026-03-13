#include <kernel/alloc.h>
#include <kernel/assert.h>
#include <kernel/cmdline.h>
#include <kernel/init.h>
#include <kernel/print.h>
#include <kernel/utils.h>
#include <string.h>
#include "limine.h"

[[__initdata_sorted("limine.0")]]
static volatile uint64_t limine_start[] = LIMINE_REQUESTS_START_MARKER;
[[__initdata_sorted("limine.1")]]
static volatile uint64_t limine_revision[] = LIMINE_BASE_REVISION(4);
[[__initdata_sorted("limine.2")]]
static volatile uint64_t limine_end[] = LIMINE_REQUESTS_END_MARKER;

#define LIMINE_REQUEST(request, tag, rev) \
    [[__initdata_sorted("limine.1")]] \
    static volatile struct limine_##request request = { \
        .id = tag, \
        .revision = rev, \
        .response = nullptr, \
    }

LIMINE_REQUEST(memmap_request, LIMINE_MEMMAP_REQUEST_ID, 0);
LIMINE_REQUEST(hhdm_request, LIMINE_HHDM_REQUEST_ID, 0);
LIMINE_REQUEST(executable_address_request, LIMINE_EXECUTABLE_ADDRESS_REQUEST_ID, 0);
LIMINE_REQUEST(executable_file_request, LIMINE_EXECUTABLE_FILE_REQUEST_ID, 0);
LIMINE_REQUEST(framebuffer_request, LIMINE_FRAMEBUFFER_REQUEST_ID, 1);
LIMINE_REQUEST(module_request, LIMINE_MODULE_REQUEST_ID, 0);
LIMINE_REQUEST(dtb_request, LIMINE_DTB_REQUEST_ID, 0);
LIMINE_REQUEST(rsdp_request, LIMINE_RSDP_REQUEST_ID, 0);

[[__initdata]]
static struct phys_mem mem[128];
[[__initdata]]
static struct boot_file files[8];

[[__init]]
void kernel_entry() {
    kernel_early_init();

    struct boot_info info;

    struct limine_memmap_response* mm_res = memmap_request.response;
    struct limine_module_response* module_res = module_request.response;
    struct limine_executable_address_response* exec_res = executable_address_request.response;
    struct limine_executable_file_response* exec_file_res = executable_file_request.response;

    info.cmdline = exec_file_res->executable_file->string;

    info.num_mem_maps = MIN(ARRAY_SIZE(mem), mm_res->entry_count);
    for (size_t i = 0; i < info.num_mem_maps; i++) {
        mem[i].address = mm_res->entries[i]->base;
        mem[i].length = mm_res->entries[i]->length;

        switch (mm_res->entries[i]->type) {
        case LIMINE_MEMMAP_USABLE:
            mem[i].usage = PHYS_USABLE;
            break;
        case LIMINE_MEMMAP_ACPI_RECLAIMABLE:
        case LIMINE_MEMMAP_BOOTLOADER_RECLAIMABLE:
            mem[i].usage = PHYS_RECLAIMABLE;
            break;
        case LIMINE_MEMMAP_EXECUTABLE_AND_MODULES:
            mem[i].usage = PHYS_STATIC;
            break;
        default:
            mem[i].usage = PHYS_RESERVED;
            break;
        }
    }

    if (module_res) {
        info.files = files;
        info.num_files = MIN(ARRAY_SIZE(files), module_res->module_count);
        for (size_t i = 0; i < info.num_files; i++) {
            files[i].data = (phys_t)module_res->modules[i]->address - hhdm_request.response->offset;
            files[i].length = module_res->modules[i]->size;
            strncpy(files[i].path, module_res->modules[i]->path, sizeof(files[i].path));
        }
    }

    info.mem_map = mem;
    info.phys_base = (phys_t)exec_res->physical_base;
    info.virt_base = (uintptr_t)exec_res->virtual_base;
    info.hhdm_base = (uintptr_t)hhdm_request.response->offset;

    info.rsdp = (phys_t)rsdp_request.response->address - info.hhdm_base;

    kernel_main(&info);
}
