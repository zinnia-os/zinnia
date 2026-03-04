#pragma once

#include <zinnia/mem.h>
#include <zinnia/status.h>
#include <kernel/dlist.h>
#include <kernel/mutex.h>
#include <kernel/pmap.h>
#include <kernel/refcount.h>
#include <kernel/vmo.h>

struct vmap {
    // Mapping, protection, etc. mutex.
    struct mutex mutex;
    // Doubly-linked list of `struct vmap_entry`.
    struct dlist entries;
};

// A contiguous mapping with the same mapping and protection mode flags.
struct vmap_entry {
    // Doubly-linked list node.
    struct dlist_node node;
    // Protection and mapping mode flags.
    enum zn_vm_flags flags;
    // Memory object mapped in this location.
    // Reference-counted `struct vmo`.
    struct rc* vmo;
    // Mapping base virtual address.
    // Always page-aligned.
    uintptr_t vaddr;
    // Mapping size.
    // Always a multiple of the page size.
    size_t size;
    // Pages of physical memory currently allocated to this mapping.
    // Reference-counted array of `struct page *`.
    struct rc* pages;
};

// Virtual address space.
struct vmspace {
    // Architecture-specific virtual memory map implementation.
    struct pmap pmap;
    // Generic memory map data.
    struct vmap map;
};

// Try to create a new, empty virtual address space.
zn_status_t vmspace_new(struct vmspace** out);
// Delete a virtual address space.
void vmspace_delete(struct vmspace* vm);

/*
// Create a new mapping.
zn_status_t vmspace_map(
    struct vmspace* vm,
    uintptr_t vaddr,
    size_t size,
    enum zn_vm_flags flags,
    struct rc* vmo,
    uintptr_t offset
);
*/
// Remove any mappings in the given address range.
// May fail if there is no memory and a mapping needs to be split.
zn_status_t vmspace_unmap(struct vmspace* vm, uintptr_t vaddr, size_t size);
// Change the protection of an existing mapping.
// May fail if there is no memory and a mapping needs to be split.
zn_status_t vmspace_protect(struct vmspace* vm, uintptr_t vaddr, size_t size, enum zn_vm_flags flags);

// Try to handle a page fault.
bool vmspace_page_fault(struct vmspace* vm, uintptr_t vaddr, enum zn_vm_flags access_type);

zn_status_t vmspace_map_vmo(
    struct vmspace* vm,
    struct vmo* vmo,
    uintptr_t addr,
    size_t len,
    enum zn_vm_flags flags,
    uintptr_t vmo_offset
);

extern struct vmspace kernel_vas;
