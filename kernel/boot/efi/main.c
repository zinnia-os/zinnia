// SPDX-License-Identifier: BSD-3-Clause

#include <boot/efi/stub.h>
#include <string.h>

constexpr size_t MEM_MAP_SLACK = 64;

struct efi_boot_services* bs;

bool loader_alloc(enum phys_mem_usage usage, size_t size, uint64_t* out) {
    enum efi_memory_type efi_type = EFI_LOADER_DATA;
    if (usage == PHYS_STATIC)
        efi_type = MEM_STATIC;
    else if (usage == PHYS_RECLAIMABLE)
        efi_type = MEM_RECLAIMABLE;

    const size_t pages = DIV_CEIL(size, PAGE_SIZE);
    if (bs->allocate_pages(EFI_ALLOCATE_ANY_PAGES, efi_type, pages, out))
        return false;
    memset((void*)*out, 0, pages * PAGE_SIZE);
    return true;
}

void* loader_ptr(uint64_t phys) {
    return (void*)phys;
}

static efi_status open_file_on(efi_handle handle, const char16* path, struct efi_file_protocol** out) {
    struct efi_simple_file_system_protocol* fs;
    efi_status status = bs->handle_protocol(handle, &EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID, (void**)&fs);
    if (status)
        return status;

    struct efi_file_protocol* root;
    status = fs->open_volume(fs, &root);
    if (status)
        return status;

    status = root->open(root, out, path, EFI_FILE_MODE_READ, 0);
    if (status && path[0] == '\\')
        status = root->open(root, out, path + 1, EFI_FILE_MODE_READ, 0);
    root->close(root);
    return status;
}

static efi_status read_file(struct efi_file_protocol* file, enum phys_mem_usage usage, uint64_t* base, uint64_t* size) {
    uint64_t total;
    uint64_t done = 0;
    efi_status status = file->set_position(file, UINT64_MAX);
    if (!status)
        status = file->get_position(file, &total);
    if (!status)
        status = file->set_position(file, 0);
    if (!status && !loader_alloc(usage, total ? total : 1, base))
        status = EFI_OUT_OF_RESOURCES;
    while (!status && done < total) {
        size_t chunk = total - done;
        status = file->read(file, &chunk, (void*)(*base + done));
        if (!chunk)
            break;
        done += chunk;
    }
    file->close(file);
    *size = done;
    return status;
}

static void color_from_mask(uint32_t mask, struct boot_color* out) {
    if (!mask)
        return;
    out->shift = __builtin_ctz(mask);
    out->size = __builtin_ctzll(~(uint64_t)(mask >> out->shift));
}

static enum phys_mem_usage convert_mem_type(uint32_t type) {
    switch (type) {
    case EFI_LOADER_CODE:
    case EFI_LOADER_DATA:
    case EFI_BOOT_SERVICES_CODE:
    case EFI_BOOT_SERVICES_DATA:
    case EFI_CONVENTIONAL_MEMORY:
        return PHYS_USABLE;
    case EFI_ACPI_RECLAIM_MEMORY:
    case MEM_RECLAIMABLE:
        return PHYS_RECLAIMABLE;
    case MEM_STATIC:
        return PHYS_STATIC;
    default:
        return PHYS_RESERVED;
    }
}

efi_status efi_main(efi_handle image_handle, struct efi_system_table* st) {
    bs = st->boot_services;

    // Check if we support this configuration.
    if (!arch_check_cpu())
        return EFI_UNSUPPORTED;

    struct efi_loaded_image_protocol* image;
    efi_status status = bs->handle_protocol(image_handle, &EFI_LOADED_IMAGE_PROTOCOL_GUID, (void**)&image);
    if (status)
        return status;

    struct kernel_image kernel;
    if (!loader_load_kernel(&kernel, "EFISTUB"))
        return EFI_LOAD_ERROR;
    struct boot_info* info = kernel.info;

    // Reserve room for the memory map.
    size_t map_size = 0;
    size_t map_key;
    size_t desc_size;
    uint32_t desc_version;
    status = bs->get_memory_map(&map_size, nullptr, &map_key, &desc_size, &desc_version);
    if (status != EFI_BUFFER_TOO_SMALL)
        return status;

    const size_t map_capacity = map_size + MEM_MAP_SLACK * desc_size;
    uint64_t map;
    if (!loader_alloc(PHYS_USABLE, map_capacity, &map))
        return EFI_OUT_OF_RESOURCES;

    uint64_t mem;
    if (!loader_alloc(PHYS_USABLE, map_capacity / desc_size * sizeof(struct phys_mem), &mem))
        return EFI_OUT_OF_RESOURCES;

    // Use the load options as the command line.
    const char16* opts = image->load_options;
    const size_t opts_len = image->load_options_size / sizeof(char16);
    size_t cmdline_len = 0;
    for (size_t i = 0; opts && i < opts_len && opts[i]; i++) {
        if (opts[i] < 0x20 || opts[i] > 0x7e || cmdline_len + 1 >= sizeof(info->cmdline)) {
            cmdline_len = 0;
            break;
        }
        info->cmdline[cmdline_len++] = (char)opts[i];
    }
    info->cmdline[cmdline_len] = 0;

    // Without load options, fall back to a command line file on the boot device.
    struct efi_file_protocol* cmdline_file;
    if (!cmdline_len && !open_file_on(image->device_handle, u"\\EFI\\zinnia\\cmdline", &cmdline_file)) {
        uint64_t base;
        uint64_t size;
        status = read_file(cmdline_file, PHYS_USABLE, &base, &size);
        if (status)
            return status;

        const char* text = (const char*)base;
        while (cmdline_len < size && text[cmdline_len] && text[cmdline_len] != '\n' && text[cmdline_len] != '\r') {
            if (text[cmdline_len] < 0x20 || text[cmdline_len] > 0x7e)
                return EFI_INVALID_PARAMETER;
            if (cmdline_len + 1 >= sizeof(info->cmdline))
                return EFI_BUFFER_TOO_SMALL;
            info->cmdline[cmdline_len] = text[cmdline_len];
            cmdline_len++;
        }
        while (cmdline_len && info->cmdline[cmdline_len - 1] == ' ')
            cmdline_len--;
        info->cmdline[cmdline_len] = 0;
    }

    // Load every initrd= file.
    const char* cmdline = info->cmdline;
    for (size_t i = 0; cmdline[i];) {
        if (cmdline[i] == ' ') {
            i++;
            continue;
        }

        const size_t start = i;
        bool quoted = false;
        while (cmdline[i] && (quoted || cmdline[i] != ' ')) {
            if (cmdline[i] == '"')
                quoted = !quoted;
            i++;
        }
        const char* token = cmdline + start;
        const size_t len = i - start;
        if (len <= 7 || memcmp(token, "initrd=", 7))
            continue;

        const size_t path_len = len - 7;
        if (path_len >= BOOT_PATH_MAX)
            return EFI_INVALID_PARAMETER;
        char path[BOOT_PATH_MAX];
        char16 wide[BOOT_PATH_MAX];
        for (size_t j = 0; j < path_len; j++) {
            path[j] = token[7 + j];
            wide[j] = path[j] == '/' ? '\\' : (char16)path[j];
        }
        path[path_len] = 0;
        wide[path_len] = 0;

        // Try the boot device first, then all other file systems.
        struct efi_file_protocol* file;
        status = open_file_on(image->device_handle, wide, &file);
        size_t num_handles;
        efi_handle* handles;
        if (status &&
            !bs->locate_handle_buffer(
                EFI_LOCATE_BY_PROTOCOL,
                &EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID,
                nullptr,
                &num_handles,
                &handles
            )) {
            for (size_t j = 0; j < num_handles && status; j++) {
                if (handles[j] != image->device_handle)
                    status = open_file_on(handles[j], wide, &file);
            }
            bs->free_pool(handles);
        }
        if (status)
            return status;

        uint64_t base;
        uint64_t size;
        status = read_file(file, PHYS_STATIC, &base, &size);
        if (status)
            return status;

        if (!loader_add_file(&kernel, path, base, size))
            return EFI_OUT_OF_RESOURCES;
    }

    // Framebuffer
    struct efi_graphics_output_protocol* gop;
    if (!bs->locate_protocol(&EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID, nullptr, (void**)&gop) && gop && gop->mode &&
        gop->mode->info) {
        const struct efi_gop_mode_info* mode = gop->mode->info;
        struct boot_framebuffer* fb = &info->fb;
        switch (mode->pixel_format) {
        case EFI_PIXEL_RGBX_8:
            fb->bytes_per_pixel = 4;
            fb->red = (struct boot_color){0, 8};
            fb->green = (struct boot_color){8, 8};
            fb->blue = (struct boot_color){16, 8};
            break;
        case EFI_PIXEL_BGRX_8:
            fb->bytes_per_pixel = 4;
            fb->red = (struct boot_color){16, 8};
            fb->green = (struct boot_color){8, 8};
            fb->blue = (struct boot_color){0, 8};
            break;
        case EFI_PIXEL_BIT_MASK: {
            const struct efi_pixel_bitmask* m = &mode->pixel_information;
            const uint32_t all = m->red | m->green | m->blue | m->reserved;
            if (!all)
                break;
            fb->bytes_per_pixel = (32 - __builtin_clz(all) + 7) / 8;
            color_from_mask(m->red, &fb->red);
            color_from_mask(m->green, &fb->green);
            color_from_mask(m->blue, &fb->blue);
            break;
        }
        default:
            break;
        }

        if (fb->bytes_per_pixel) {
            fb->base = gop->mode->frame_buffer_base;
            fb->width = mode->horizontal_resolution;
            fb->height = mode->vertical_resolution;
            fb->pitch = mode->pixels_per_scan_line * fb->bytes_per_pixel;
        }
    }

    // Find the RSDP.
    for (size_t i = 0; i < st->number_of_table_entries; i++) {
        const struct efi_configuration_table* table = &st->configuration_table[i];
        if (!memcmp(&table->vendor_guid, &EFI_ACPI_20_TABLE_GUID, sizeof(struct efi_guid))) {
            info->rsdp = (uint64_t)table->vendor_table;
            break;
        }
        if (!memcmp(&table->vendor_guid, &EFI_ACPI_10_TABLE_GUID, sizeof(struct efi_guid)))
            info->rsdp = (uint64_t)table->vendor_table;
    }

    // Map everything up to the highest non-reserved address.
    map_size = map_capacity;
    status = bs->get_memory_map(&map_size, (struct efi_memory_descriptor*)map, &map_key, &desc_size, &desc_version);
    if (status)
        return status;

    uint64_t top = 0;
    for (size_t off = 0; off + desc_size <= map_size; off += desc_size) {
        const struct efi_memory_descriptor* desc = (const struct efi_memory_descriptor*)(map + off);
        if (convert_mem_type(desc->type) != PHYS_RESERVED)
            top = MAX(top, desc->physical_start + desc->number_of_pages * PAGE_SIZE);
    }

    uint64_t root;
    if (!loader_page_tables(&kernel, top, (uint64_t)arch_enter_kernel, &root))
        return EFI_OUT_OF_RESOURCES;

    // Exit boot services.
    for (size_t attempt = 0;; attempt++) {
        map_size = map_capacity;
        status = bs->get_memory_map(&map_size, (struct efi_memory_descriptor*)map, &map_key, &desc_size, &desc_version);
        if (status)
            return status;
        status = bs->exit_boot_services(image_handle, map_key);
        if (!status)
            break;
        if (attempt == 8)
            return status;
    }

    // Hand the final memory map to the kernel.
    struct phys_mem* mem_map = (struct phys_mem*)mem;
    size_t num_mem = 0;
    for (size_t off = 0; off + desc_size <= map_size; off += desc_size) {
        const struct efi_memory_descriptor* desc = (const struct efi_memory_descriptor*)(map + off);
        mem_map[num_mem++] = (struct phys_mem){
            .address = desc->physical_start,
            .length = desc->number_of_pages * PAGE_SIZE,
            .usage = convert_mem_type(desc->type),
        };
    }
    loader_set_mem_map(&kernel, mem_map, num_mem);

    arch_enter_kernel(root, kernel.entry);
}
