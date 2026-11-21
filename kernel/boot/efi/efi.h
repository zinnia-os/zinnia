// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stddef.h>
#include <stdint.h>
#if defined(__x86_64__)
#include <boot/efi/x86_64/efi.h>
#else
#error "Unsupported architecture"
#endif

typedef uint64_t efi_status;
typedef void* efi_handle;
typedef uint16_t char16;

constexpr efi_status EFI_SUCCESS = 0;
constexpr efi_status EFI_ERROR = 1ull << 63;
constexpr efi_status EFI_LOAD_ERROR = EFI_ERROR | 1;
constexpr efi_status EFI_INVALID_PARAMETER = EFI_ERROR | 2;
constexpr efi_status EFI_UNSUPPORTED = EFI_ERROR | 3;
constexpr efi_status EFI_BUFFER_TOO_SMALL = EFI_ERROR | 5;
constexpr efi_status EFI_OUT_OF_RESOURCES = EFI_ERROR | 9;
constexpr efi_status EFI_NOT_FOUND = EFI_ERROR | 14;

struct efi_guid {
    uint32_t data1;
    uint16_t data2;
    uint16_t data3;
    uint8_t data4[8];
};

enum efi_memory_type : uint32_t {
    EFI_RESERVED_MEMORY_TYPE,
    EFI_LOADER_CODE,
    EFI_LOADER_DATA,
    EFI_BOOT_SERVICES_CODE,
    EFI_BOOT_SERVICES_DATA,
    EFI_RUNTIME_SERVICES_CODE,
    EFI_RUNTIME_SERVICES_DATA,
    EFI_CONVENTIONAL_MEMORY,
    EFI_UNUSABLE_MEMORY,
    EFI_ACPI_RECLAIM_MEMORY,
    EFI_ACPI_MEMORY_NVS,
    EFI_MEMORY_MAPPED_IO,
    EFI_MEMORY_MAPPED_IO_PORT_SPACE,
    EFI_PAL_CODE,
    EFI_PERSISTENT_MEMORY,
    EFI_UNACCEPTED_MEMORY,
};

enum efi_allocate_type : uint32_t {
    EFI_ALLOCATE_ANY_PAGES,
    EFI_ALLOCATE_MAX_ADDRESS,
    EFI_ALLOCATE_ADDRESS,
};

enum efi_locate_search_type : uint32_t {
    EFI_LOCATE_ALL_HANDLES,
    EFI_LOCATE_BY_REGISTER_NOTIFY,
    EFI_LOCATE_BY_PROTOCOL,
};

struct efi_memory_descriptor {
    uint32_t type;
    uint64_t physical_start;
    uint64_t virtual_start;
    uint64_t number_of_pages;
    uint64_t attribute;
};

struct efi_table_header {
    uint64_t signature;
    uint32_t revision;
    uint32_t header_size;
    uint32_t crc32;
    uint32_t reserved;
};

struct efi_simple_text_output_protocol {
    void* reset;
    efi_status(EFIAPI* output_string)(struct efi_simple_text_output_protocol* self, const char16* string);
};

struct efi_boot_services {
    struct efi_table_header hdr;
    void* raise_tpl;
    void* restore_tpl;
    efi_status(EFIAPI* allocate_pages)(
        enum efi_allocate_type type,
        enum efi_memory_type memory_type,
        size_t pages,
        uint64_t* memory
    );
    efi_status(EFIAPI* free_pages)(uint64_t memory, size_t pages);
    efi_status(EFIAPI* get_memory_map)(
        size_t* map_size,
        struct efi_memory_descriptor* map,
        size_t* map_key,
        size_t* descriptor_size,
        uint32_t* descriptor_version
    );
    efi_status(EFIAPI* allocate_pool)(enum efi_memory_type pool_type, size_t size, void** buffer);
    efi_status(EFIAPI* free_pool)(void* buffer);
    void* create_event;
    void* set_timer;
    void* wait_for_event;
    void* signal_event;
    void* close_event;
    void* check_event;
    void* install_protocol_interface;
    void* reinstall_protocol_interface;
    void* uninstall_protocol_interface;
    efi_status(EFIAPI* handle_protocol)(efi_handle handle, const struct efi_guid* protocol, void** interface);
    void* reserved;
    void* register_protocol_notify;
    void* locate_handle;
    void* locate_device_path;
    void* install_configuration_table;
    void* load_image;
    void* start_image;
    void* exit;
    void* unload_image;
    efi_status(EFIAPI* exit_boot_services)(efi_handle image_handle, size_t map_key);
    void* get_next_monotonic_count;
    void* stall;
    void* set_watchdog_timer;
    void* connect_controller;
    void* disconnect_controller;
    void* open_protocol;
    void* close_protocol;
    void* open_protocol_information;
    void* protocols_per_handle;
    efi_status(EFIAPI* locate_handle_buffer)(
        enum efi_locate_search_type search_type,
        const struct efi_guid* protocol,
        void* search_key,
        size_t* no_handles,
        efi_handle** buffer
    );
    efi_status(EFIAPI* locate_protocol)(const struct efi_guid* protocol, void* registration, void** interface);
};

struct efi_configuration_table {
    struct efi_guid vendor_guid;
    void* vendor_table;
};

struct efi_system_table {
    struct efi_table_header hdr;
    char16* firmware_vendor;
    uint32_t firmware_revision;
    efi_handle console_in_handle;
    void* con_in;
    efi_handle console_out_handle;
    struct efi_simple_text_output_protocol* con_out;
    efi_handle standard_error_handle;
    void* std_err;
    void* runtime_services;
    struct efi_boot_services* boot_services;
    size_t number_of_table_entries;
    struct efi_configuration_table* configuration_table;
};

struct efi_loaded_image_protocol {
    uint32_t revision;
    efi_handle parent_handle;
    struct efi_system_table* system_table;
    efi_handle device_handle;
    void* file_path;
    void* reserved;
    uint32_t load_options_size;
    void* load_options;
    void* image_base;
    uint64_t image_size;
    enum efi_memory_type image_code_type;
    enum efi_memory_type image_data_type;
    void* unload;
};

constexpr uint64_t EFI_FILE_MODE_READ = 1;

struct efi_file_protocol {
    uint64_t revision;
    efi_status(EFIAPI* open)(
        struct efi_file_protocol* self,
        struct efi_file_protocol** new_handle,
        const char16* file_name,
        uint64_t open_mode,
        uint64_t attributes
    );
    efi_status(EFIAPI* close)(struct efi_file_protocol* self);
    void* delete;
    efi_status(EFIAPI* read)(struct efi_file_protocol* self, size_t* buffer_size, void* buffer);
    void* write;
    efi_status(EFIAPI* get_position)(struct efi_file_protocol* self, uint64_t* position);
    efi_status(EFIAPI* set_position)(struct efi_file_protocol* self, uint64_t position);
};

struct efi_simple_file_system_protocol {
    uint64_t revision;
    efi_status(EFIAPI* open_volume)(struct efi_simple_file_system_protocol* self, struct efi_file_protocol** root);
};

enum efi_pixel_format : uint32_t {
    EFI_PIXEL_RGBX_8,
    EFI_PIXEL_BGRX_8,
    EFI_PIXEL_BIT_MASK,
    EFI_PIXEL_BLT_ONLY,
};

struct efi_pixel_bitmask {
    uint32_t red;
    uint32_t green;
    uint32_t blue;
    uint32_t reserved;
};

struct efi_gop_mode_info {
    uint32_t version;
    uint32_t horizontal_resolution;
    uint32_t vertical_resolution;
    enum efi_pixel_format pixel_format;
    struct efi_pixel_bitmask pixel_information;
    uint32_t pixels_per_scan_line;
};

struct efi_gop_mode {
    uint32_t max_mode;
    uint32_t mode;
    struct efi_gop_mode_info* info;
    size_t size_of_info;
    uint64_t frame_buffer_base;
    size_t frame_buffer_size;
};

struct efi_graphics_output_protocol {
    void* query_mode;
    void* set_mode;
    void* blt;
    struct efi_gop_mode* mode;
};

static const struct efi_guid EFI_LOADED_IMAGE_PROTOCOL_GUID =
    {0x5b1b'31a1, 0x9562, 0x11d2, {0x8e, 0x3f, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b}};
static const struct efi_guid EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID =
    {0x964e'5b22, 0x6459, 0x11d2, {0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b}};
static const struct efi_guid EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID =
    {0x9042'a9de, 0x23dc, 0x4a38, {0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a}};
static const struct efi_guid EFI_ACPI_20_TABLE_GUID =
    {0x8868'e871, 0xe4f1, 0x11d3, {0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81}};
static const struct efi_guid EFI_ACPI_10_TABLE_GUID =
    {0xeb9d'2d30, 0x2d88, 0x11d3, {0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d}};
static const struct efi_guid EFI_DTB_TABLE_GUID =
    {0xb1b6'21d5, 0xf19c, 0x41a5, {0x83, 0x0b, 0xd9, 0x15, 0x2c, 0x69, 0xaa, 0xe0}};
