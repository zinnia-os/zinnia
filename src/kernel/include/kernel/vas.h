#pragma once

#include <zinnia/mem.h>
#include <zinnia/status.h>
#include <kernel/mmu.h>
#include <kernel/vmo.h>

// Virtual address space.
struct vas {
    struct page_table pt;
};

zn_status_t vas_new(struct vas** out);
zn_status_t vas_map_vmo(
    struct vas* vas,
    struct vmo* vmo,
    uintptr_t addr,
    size_t len,
    enum zn_vm_flags flags,
    uintptr_t vmo_offset
);

extern struct vas kernel_vas;
