// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stddef.h>
#include <stdint.h>

constexpr uint16_t PE_DOS_MAGIC = 0x5a4d;
constexpr uint32_t PE_SIGNATURE = 0x0000'4550;
constexpr uint16_t PE_OPTIONAL_MAGIC_PE32_PLUS = 0x020b;
constexpr uint16_t PE_SUBSYSTEM_EFI_APPLICATION = 10;
constexpr size_t PE_NUM_DATA_DIRECTORIES = 16;
constexpr size_t PE_DIRECTORY_BASE_RELOC = 5;

enum pe_file_characteristics : uint16_t {
    PE_FILE_EXECUTABLE_IMAGE = 0x0002,
    PE_FILE_LINE_NUMS_STRIPPED = 0x0004,
    PE_FILE_DEBUG_STRIPPED = 0x0200,
};

enum pe_dll_characteristics : uint16_t {
    PE_DLL_NX_COMPAT = 0x0100,
};

enum pe_section_characteristics : uint32_t {
    PE_SCN_CNT_CODE = 0x0000'0020,
    PE_SCN_CNT_INITIALIZED_DATA = 0x0000'0040,
    PE_SCN_MEM_DISCARDABLE = 0x0200'0000,
    PE_SCN_MEM_EXECUTE = 0x2000'0000,
    PE_SCN_MEM_READ = 0x4000'0000,
    PE_SCN_MEM_WRITE = 0x8000'0000,
};

struct pe_dos_header {
    uint16_t e_magic;
    uint16_t e_cblp;
    uint16_t e_cp;
    uint16_t e_crlc;
    uint16_t e_cparhdr;
    uint16_t e_minalloc;
    uint16_t e_maxalloc;
    uint16_t e_ss;
    uint16_t e_sp;
    uint16_t e_csum;
    uint16_t e_ip;
    uint16_t e_cs;
    uint16_t e_lfarlc;
    uint16_t e_ovno;
    uint16_t e_res[4];
    uint16_t e_oemid;
    uint16_t e_oeminfo;
    uint16_t e_res2[10];
    uint32_t e_lfanew;
};

struct pe_coff_header {
    uint16_t machine;
    uint16_t number_of_sections;
    uint32_t time_date_stamp;
    uint32_t pointer_to_symbol_table;
    uint32_t number_of_symbols;
    uint16_t size_of_optional_header;
    uint16_t characteristics;
};

struct pe_data_directory {
    union {
        struct {
            uint32_t virtual_address;
            uint32_t size;
        };
        uint64_t layout;
    };
};

struct pe_optional_header64 {
    union {
        struct {
            uint16_t magic;
            uint8_t major_linker_version;
            uint8_t minor_linker_version;
            uint32_t size_of_code;
        };
        uint64_t magic_and_code_size;
    };
    union {
        struct {
            uint32_t size_of_initialized_data;
            uint32_t size_of_uninitialized_data;
        };
        uint64_t data_sizes;
    };
    union {
        struct {
            uint32_t address_of_entry_point;
            uint32_t base_of_code;
        };
        uint64_t entry_and_code_base;
    };
    uint64_t image_base;
    uint32_t section_alignment;
    uint32_t file_alignment;
    uint16_t major_operating_system_version;
    uint16_t minor_operating_system_version;
    uint16_t major_image_version;
    uint16_t minor_image_version;
    uint16_t major_subsystem_version;
    uint16_t minor_subsystem_version;
    uint32_t win32_version_value;
    union {
        struct {
            uint32_t size_of_image;
            uint32_t size_of_headers;
        };
        uint64_t image_and_header_sizes;
    };
    uint32_t checksum;
    uint16_t subsystem;
    uint16_t dll_characteristics;
    uint64_t size_of_stack_reserve;
    uint64_t size_of_stack_commit;
    uint64_t size_of_heap_reserve;
    uint64_t size_of_heap_commit;
    uint32_t loader_flags;
    uint32_t number_of_rva_and_sizes;
    struct pe_data_directory data_directories[PE_NUM_DATA_DIRECTORIES];
};

struct pe_section_header {
    char name[8];
    union {
        struct {
            uint32_t virtual_size;
            uint32_t virtual_address;
        };
        uint64_t virtual_layout;
    };
    union {
        struct {
            uint32_t size_of_raw_data;
            uint32_t pointer_to_raw_data;
        };
        uint64_t raw_layout;
    };
    uint32_t pointer_to_relocations;
    uint32_t pointer_to_line_numbers;
    uint16_t number_of_relocations;
    uint16_t number_of_line_numbers;
    uint32_t characteristics;
};

struct pe_base_reloc_block {
    uint32_t page_rva;
    uint32_t block_size;
    uint16_t entries[2];
};

static_assert(sizeof(struct pe_dos_header) == 64);
static_assert(sizeof(struct pe_coff_header) == 20);
static_assert(sizeof(struct pe_optional_header64) == 240);
static_assert(offsetof(struct pe_optional_header64, size_of_image) == 56);
static_assert(offsetof(struct pe_optional_header64, data_directories) == 112);
static_assert(sizeof(struct pe_section_header) == 40);
static_assert(sizeof(struct pe_base_reloc_block) == 12);
