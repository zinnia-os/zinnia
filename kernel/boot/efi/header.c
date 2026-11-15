// SPDX-License-Identifier: BSD-3-Clause

#include <boot/efi/efi.h>
#include <boot/efi/pe.h>
#include <lib/compiler.h>
#include <stddef.h>
#include <stdint.h>

extern const char __pe_code_size[];
extern const char __pe_data_sizes[];
extern const char __pe_entry[];
extern const char __pe_image_sizes[];
extern const char __pe_base_relocs[];
extern const char __pe_text_virtual[];
extern const char __pe_text_raw[];
extern const char __pe_reloc_virtual[];
extern const char __pe_reloc_raw[];
extern const char __pe_rodata_virtual[];
extern const char __pe_rodata_raw[];
extern const char __pe_data_virtual[];
extern const char __pe_data_raw[];

struct efistub_pe_header {
    struct pe_dos_header dos;
    uint32_t signature;
    struct pe_coff_header coff;
    struct pe_optional_header64 optional;
    struct pe_section_header sections[4];
};

static_assert(offsetof(struct efistub_pe_header, optional) % 8 == 0);
static_assert(offsetof(struct efistub_pe_header, sections) % 8 == 0);

[[__used, __section(".header")]]
const struct efistub_pe_header efistub_pe_header = {
    .dos =
        {
            .e_magic = PE_DOS_MAGIC,
            .e_lfanew = offsetof(struct efistub_pe_header, signature),
        },
    .signature = PE_SIGNATURE,
    .coff =
        {
            .machine = EFI_ARCH_PE_MACHINE,
            .number_of_sections = 4,
            .size_of_optional_header = sizeof(struct pe_optional_header64),
            .characteristics = PE_FILE_EXECUTABLE_IMAGE | PE_FILE_LINE_NUMS_STRIPPED | PE_FILE_DEBUG_STRIPPED,
        },
    .optional =
        {
            .magic_and_code_size = (uintptr_t)__pe_code_size + PE_OPTIONAL_MAGIC_PE32_PLUS,
            .data_sizes = (uintptr_t)__pe_data_sizes,
            .entry_and_code_base = (uintptr_t)__pe_entry,
            .section_alignment = 0x1000,
            .file_alignment = 0x1000,
            .image_and_header_sizes = (uintptr_t)__pe_image_sizes,
            .subsystem = PE_SUBSYSTEM_EFI_APPLICATION,
            .dll_characteristics = PE_DLL_NX_COMPAT,
            .number_of_rva_and_sizes = PE_NUM_DATA_DIRECTORIES,
            .data_directories =
                {
                    [PE_DIRECTORY_BASE_RELOC] = {.layout = (uintptr_t)__pe_base_relocs},
                },
        },
    .sections = {
        {
            .name = ".text",
            .virtual_layout = (uintptr_t)__pe_text_virtual,
            .raw_layout = (uintptr_t)__pe_text_raw,
            .characteristics = PE_SCN_CNT_CODE | PE_SCN_MEM_EXECUTE | PE_SCN_MEM_READ,
        },
        {
            .name = ".reloc",
            .virtual_layout = (uintptr_t)__pe_reloc_virtual,
            .raw_layout = (uintptr_t)__pe_reloc_raw,
            .characteristics = PE_SCN_CNT_INITIALIZED_DATA | PE_SCN_MEM_DISCARDABLE | PE_SCN_MEM_READ,
        },
        {
            .name = ".rdata",
            .virtual_layout = (uintptr_t)__pe_rodata_virtual,
            .raw_layout = (uintptr_t)__pe_rodata_raw,
            .characteristics = PE_SCN_CNT_INITIALIZED_DATA | PE_SCN_MEM_READ,
        },
        {
            .name = ".data",
            .virtual_layout = (uintptr_t)__pe_data_virtual,
            .raw_layout = (uintptr_t)__pe_data_raw,
            .characteristics = PE_SCN_CNT_INITIALIZED_DATA | PE_SCN_MEM_READ | PE_SCN_MEM_WRITE,
        },
    },
};

[[__used, __section(".reloc")]]
const struct pe_base_reloc_block efistub_base_relocs = {
    .page_rva = 0,
    .block_size = sizeof(struct pe_base_reloc_block),
};
