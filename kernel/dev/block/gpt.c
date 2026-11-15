// SPDX-License-Identifier: BSD-3-Clause

#include <dev/block/blockdev.h>
#include <mem/alloc.h>
#include <string.h>

struct [[__packed]] gpt_header {
    char signature[8];
    uint32_t revision;
    uint32_t header_size;
    uint32_t header_crc32;
    uint32_t reserved;
    uint64_t my_lba;
    uint64_t alternate_lba;
    uint64_t first_usable_lba;
    uint64_t last_usable_lba;
    struct guid disk_guid;
    uint64_t partition_entry_lba;
    uint32_t num_partition_entries;
    uint32_t partition_entry_size;
    uint32_t partition_entry_array_crc32;
};

struct [[__packed]] gpt_entry {
    struct guid type_guid;
    struct guid unique_guid;
    uint64_t start_lba;
    uint64_t end_lba;
    uint64_t attributes;
    uint16_t name[36];
};

constexpr size_t GPT_MAX_ENTRIES = 1024;

errno_t gpt_scan(struct blockdev* dev, struct gpt_partition** out, size_t* out_count, size_t* out_capacity) {
    struct gpt_header header;
    size_t read;
    errno_t status = vmo_read(&dev->cache, dev->lba_size, &header, sizeof(header), &read);
    if (status)
        return status;
    if (read != sizeof(header) || memcmp(header.signature, "EFI PART", 8) != 0)
        return ENODATA;
    if (header.partition_entry_size < sizeof(struct gpt_entry) || header.num_partition_entries > GPT_MAX_ENTRIES)
        return EINVAL;

    const size_t capacity = MAX(header.num_partition_entries, 1);
    struct gpt_partition* parts = mem_alloc(capacity * sizeof(struct gpt_partition), 0);
    if (!parts)
        return ENOMEM;

    size_t count = 0;
    const uint64_t base = header.partition_entry_lba * dev->lba_size;
    for (size_t i = 0; i < header.num_partition_entries; i++) {
        struct gpt_entry entry;
        status = vmo_read(&dev->cache, base + i * header.partition_entry_size, &entry, sizeof(entry), &read);
        if (status || read != sizeof(entry))
            break;
        if (guid_is_zero(&entry.type_guid))
            continue;

        struct gpt_partition* part = &parts[count++];
        part->type_guid = entry.type_guid;
        part->unique_guid = entry.unique_guid;
        part->start_lba = entry.start_lba;
        part->end_lba = entry.end_lba;
    }

    *out = parts;
    *out_count = count;
    *out_capacity = capacity;
    return 0;
}
