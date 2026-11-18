// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <lib/mutex.h>
#include <lib/rwlock.h>
#include <lib/tailq.h>
#include <mem/amap.h>
#include <mem/mman.h>
#include <mem/pmap.h>
#include <mem/vmo.h>

enum vma_flags : uint8_t {
    VMA_SHARED = 1 << 0,
    VMA_NEEDS_COPY = 1 << 1,
};

struct vma;

struct vma_mapping {
    size_t start_page;
    size_t end_page;
    size_t offset_page;
    struct vmo* object;
    struct amap* amap;
    size_t amap_offset;
    struct vma* vma;
    enum prot_flags prot;
    enum vma_flags flags;
    tailq_link(struct vma_mapping) link;
    tailq_link(struct vma_mapping) object_link;
};

// Virtual address space.
struct vma {
    // Architecture-specific virtual memory map implementation.
    struct pmap pmap;
    tailq_head(struct vma_mapping) mappings;
    struct rwlock lock;
    mutex_t map_lock;
    struct refcount rc;
};

// Try to create a new, empty virtual address space.
errno_t vma_new(struct vma** out);

void vma_ref(struct vma* v);
void vma_unref(struct vma* v);
errno_t vma_fork(struct vma* src, struct vma** out);

errno_t vma_map(
    struct vma* vma,
    struct vmo* vmo,
    uintptr_t addr,
    size_t len,
    enum prot_flags prot,
    enum vma_flags flags,
    uintptr_t vmo_offset
);

errno_t vma_find_free(struct vma* v, uintptr_t hint, size_t len, uintptr_t* out);

bool vma_range_mapped(struct vma* v, uintptr_t vaddr, size_t size);

// Remove any mappings in the given address range.
errno_t vma_unmap(struct vma* v, uintptr_t vaddr, size_t size);

// Change the protection of an existing mapping.
errno_t vma_protect(struct vma* v, uintptr_t vaddr, size_t size, enum prot_flags flags);

errno_t vma_write(struct vma* v, uintptr_t addr, const void* buf, size_t len);
errno_t vma_zero(struct vma* v, uintptr_t addr, size_t len);
errno_t vma_sync(struct vma* v, uintptr_t addr, size_t len);

void vma_object_remove_page(struct vmo* object, uint64_t index, phys_t phys, bool protect);

struct vma_fault_info {
    uintptr_t ip;
    uintptr_t addr;
    bool caused_by_user:1;
    bool caused_by_write:1;
    bool caused_by_fetch:1;
    bool page_was_present:1;
};

// Try to handle a page fault.
errno_t vma_page_fault(struct vma* v, const struct vma_fault_info* info);

extern struct vma kernel_vma;
