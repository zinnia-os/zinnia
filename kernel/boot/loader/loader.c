// SPDX-License-Identifier: BSD-3-Clause

#include <boot/loader/loader.h>
#include <stdcountof.h>
#include <string.h>

#if defined(__x86_64__)
#include <boot/loader/x86_64/loader_arch.h>
#else
#error "Unsupported architecture"
#endif

extern const uint8_t __kernel_start[];
extern const uint8_t __kernel_end[];

bool loader_load_kernel(struct kernel_image* out, const char* name) {
    if (!elf_load(__kernel_start, __kernel_end - __kernel_start, out))
        return false;

    struct boot_info* info = out->info;
    memcpy(info->loader, name, MIN(strlen(name), sizeof(info->loader) - 1));
    info->hhdm_base = ARCH_HHDM_BASE;
    info->phys_base = out->phys;
    info->virt_base = out->virt;
    return true;
}

bool loader_add_file(const struct kernel_image* kernel, const char* path, uint64_t base, uint64_t length) {
    struct boot_info* info = kernel->info;
    if (info->num_files == countof(info->files))
        return false;

    struct boot_file* file = &info->files[info->num_files++];
    file->data = base;
    file->length = length;
    const size_t path_len = MIN(strlen(path), sizeof(file->path) - 1);
    memcpy(file->path, path, path_len);
    file->path[path_len] = 0;
    return true;
}

void loader_set_mem_map(const struct kernel_image* kernel, struct phys_mem* map, size_t len) {
    for (size_t i = 1; i < len; i++) {
        const struct phys_mem entry = map[i];
        size_t j = i;
        while (j > 0 && map[j - 1].address > entry.address) {
            map[j] = map[j - 1];
            j--;
        }
        map[j] = entry;
    }

    size_t merged = 0;
    for (size_t i = 0; i < len; i++) {
        if (!map[i].length)
            continue;
        struct phys_mem* prev = merged ? &map[merged - 1] : nullptr;
        if (prev && prev->usage == map[i].usage && prev->address + prev->length == map[i].address)
            prev->length += map[i].length;
        else
            map[merged++] = map[i];
    }

    struct boot_info* info = kernel->info;
    info->num_mem_maps = MIN(merged, countof(info->mem_map));
    memcpy(info->mem_map, map, info->num_mem_maps * sizeof(*map));
}

bool loader_page_tables(
    const struct kernel_image* kernel,
    uint64_t mem_top,
    uint64_t trampoline_phys,
    uint64_t* out_root
) {
    uint64_t root;
    if (!loader_alloc(PHYS_RECLAIMABLE, PAGE_SIZE, &root))
        return false;

    const uint64_t trampoline_virt = ROUND_DOWN((uint64_t)arch_enter_kernel, PAGE_SIZE);
    if (!arch_paging_map(root, ARCH_HHDM_BASE, 0, ROUND_UP(mem_top, ARCH_HUGE_PAGE_SIZE), ARCH_HUGE_PAGE_SIZE) ||
        !arch_paging_map(root, kernel->virt, kernel->phys, kernel->size, PAGE_SIZE) ||
        !arch_paging_map(root, trampoline_virt, ROUND_DOWN(trampoline_phys, PAGE_SIZE), 2 * PAGE_SIZE, PAGE_SIZE))
        return false;

    *out_root = root;
    return true;
}
