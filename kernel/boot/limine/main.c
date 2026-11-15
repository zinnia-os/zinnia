// SPDX-License-Identifier: BSD-3-Clause

#include <boot/limine/limine.h>
#include <boot/loader/loader.h>
#include <lib/compiler.h>
#include <string.h>

[[__used, __section(".limine_requests_start")]]
static volatile uint64_t limine_start[] = LIMINE_REQUESTS_START_MARKER;
[[__used, __section(".limine_requests")]]
static volatile uint64_t limine_revision[] = LIMINE_BASE_REVISION(6);
[[__used, __section(".limine_requests_end")]]
static volatile uint64_t limine_end[] = LIMINE_REQUESTS_END_MARKER;

#define LIMINE_REQUEST(request, tag, rev) \
    [[__used, __section(".limine_requests")]] \
    static volatile struct limine_##request request = { \
        .id = tag, \
        .revision = rev, \
        .response = nullptr, \
    }

LIMINE_REQUEST(bootloader_info_request, LIMINE_BOOTLOADER_INFO_REQUEST_ID, 0);
LIMINE_REQUEST(memmap_request, LIMINE_MEMMAP_REQUEST_ID, 0);
LIMINE_REQUEST(hhdm_request, LIMINE_HHDM_REQUEST_ID, 0);
LIMINE_REQUEST(executable_address_request, LIMINE_EXECUTABLE_ADDRESS_REQUEST_ID, 0);
LIMINE_REQUEST(executable_file_request, LIMINE_EXECUTABLE_FILE_REQUEST_ID, 0);
LIMINE_REQUEST(framebuffer_request, LIMINE_FRAMEBUFFER_REQUEST_ID, 1);
LIMINE_REQUEST(module_request, LIMINE_MODULE_REQUEST_ID, 0);
LIMINE_REQUEST(dtb_request, LIMINE_DTB_REQUEST_ID, 0);
LIMINE_REQUEST(rsdp_request, LIMINE_RSDP_REQUEST_ID, 0);

static uint64_t hhdm_offset;
static const struct limine_memmap_entry* pool;
static uint64_t pool_static_end;
static uint64_t pool_reclaimable_start;

void* loader_ptr(uint64_t phys) {
    return (void*)(phys + hhdm_offset);
}

static uint64_t to_phys(const void* ptr) {
    const uint64_t addr = (uint64_t)ptr;
    return addr >= hhdm_offset ? addr - hhdm_offset : addr;
}

bool loader_alloc(enum phys_mem_usage usage, size_t size, uint64_t* out) {
    size = ROUND_UP(size, PAGE_SIZE);
    if (pool_reclaimable_start - pool_static_end < size)
        return false;

    if (usage == PHYS_STATIC) {
        *out = pool_static_end;
        pool_static_end += size;
    } else {
        pool_reclaimable_start -= size;
        *out = pool_reclaimable_start;
    }
    memset(loader_ptr(*out), 0, size);
    return true;
}

static enum phys_mem_usage convert_mem_type(uint64_t type) {
    switch (type) {
    case LIMINE_MEMMAP_USABLE:
        return PHYS_USABLE;
    case LIMINE_MEMMAP_ACPI_RECLAIMABLE:
    case LIMINE_MEMMAP_BOOTLOADER_RECLAIMABLE:
        return PHYS_RECLAIMABLE;
    case LIMINE_MEMMAP_EXECUTABLE_AND_MODULES:
        return PHYS_STATIC;
    default:
        return PHYS_RESERVED;
    }
}

static bool init_pool(const struct limine_memmap_response* memmap) {
    for (size_t i = 0; i < memmap->entry_count; i++) {
        const struct limine_memmap_entry* entry = memmap->entries[i];
        if (entry->type == LIMINE_MEMMAP_USABLE && (!pool || entry->length > pool->length))
            pool = entry;
    }
    if (!pool)
        return false;

    pool_static_end = pool->base;
    pool_reclaimable_start = pool->base + pool->length;
    return true;
}

static uint64_t mem_map_top(const struct limine_memmap_response* memmap) {
    uint64_t top = 0;
    for (size_t i = 0; i < memmap->entry_count; i++) {
        const struct limine_memmap_entry* entry = memmap->entries[i];
        if (convert_mem_type(entry->type) != PHYS_RESERVED)
            top = MAX(top, entry->base + entry->length);
    }
    return top;
}

static size_t convert_mem_map(const struct limine_memmap_response* memmap, struct phys_mem* out) {
    size_t n = 0;
    for (size_t i = 0; i < memmap->entry_count; i++) {
        const struct limine_memmap_entry* entry = memmap->entries[i];
        if (entry != pool) {
            out[n++] = (struct phys_mem){entry->base, entry->length, convert_mem_type(entry->type)};
            continue;
        }

        const uint64_t end = entry->base + entry->length;
        out[n++] = (struct phys_mem){entry->base, pool_static_end - entry->base, PHYS_STATIC};
        out[n++] = (struct phys_mem){pool_static_end, pool_reclaimable_start - pool_static_end, PHYS_USABLE};
        out[n++] = (struct phys_mem){pool_reclaimable_start, end - pool_reclaimable_start, PHYS_RECLAIMABLE};
    }
    return n;
}

static void find_framebuffer(struct boot_framebuffer* out) {
    const struct limine_framebuffer_response* res = framebuffer_request.response;
    if (!res || !res->framebuffer_count)
        return;

    const struct limine_framebuffer* fb = res->framebuffers[0];
    out->base = to_phys(fb->address);
    out->width = fb->width;
    out->height = fb->height;
    out->pitch = fb->pitch;
    out->bytes_per_pixel = fb->bpp / 8;
    out->red = (struct boot_color){fb->red_mask_shift, fb->red_mask_size};
    out->green = (struct boot_color){fb->green_mask_shift, fb->green_mask_size};
    out->blue = (struct boot_color){fb->blue_mask_shift, fb->blue_mask_size};
}

static bool load() {
    const struct limine_memmap_response* memmap = memmap_request.response;
    const struct limine_executable_address_response* exec = executable_address_request.response;
    if (!LIMINE_BASE_REVISION_SUPPORTED(limine_revision) || !memmap || !hhdm_request.response || !exec)
        return false;
    hhdm_offset = hhdm_request.response->offset;

    if (!arch_check_cpu() || !init_pool(memmap))
        return false;

    const struct limine_bootloader_info_response* info = bootloader_info_request.response;
    struct kernel_image kernel;
    if (!loader_load_kernel(&kernel, info ? info->name : "Limine"))
        return false;

    uint64_t mem;
    if (!loader_alloc(PHYS_RECLAIMABLE, (memmap->entry_count + 2) * sizeof(struct phys_mem), &mem))
        return false;

    const struct limine_executable_file_response* file = executable_file_request.response;
    if (file && file->executable_file->string) {
        const char* cmdline = file->executable_file->string;
        memcpy(kernel.info->cmdline, cmdline, MIN(strlen(cmdline), sizeof(kernel.info->cmdline) - 1));
    }

    const struct limine_module_response* modules = module_request.response;
    for (size_t i = 0; modules && i < modules->module_count; i++) {
        const struct limine_file* module = modules->modules[i];
        if (!loader_add_file(&kernel, module->path, to_phys(module->address), module->size))
            return false;
    }

    find_framebuffer(&kernel.info->fb);
    if (rsdp_request.response)
        kernel.info->rsdp = to_phys(rsdp_request.response->address);
    if (dtb_request.response && dtb_request.response->dtb_ptr)
        kernel.info->dtb = to_phys(dtb_request.response->dtb_ptr);

    const uint64_t trampoline = (uint64_t)arch_enter_kernel;
    uint64_t root;
    if (!loader_page_tables(
            &kernel,
            mem_map_top(memmap),
            exec->physical_base + (trampoline - exec->virtual_base),
            &root
        ))
        return false;

    struct phys_mem* mem_map = loader_ptr(mem);
    loader_set_mem_map(&kernel, mem_map, convert_mem_map(memmap, mem_map));
    arch_enter_kernel(root, kernel.entry);
}

[[noreturn]]
void loader_main() {
    load();
    for (;;)
        asm volatile("");
}
