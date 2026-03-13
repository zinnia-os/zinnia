#pragma once

#include <kernel/mutex.h>
#include <kernel/pmap.h>
#include <kernel/vmobject.h>
#include <uapi/errno.h>
#include <uapi/mman.h>

struct vmap {
    struct mutex mutex;
};

// Virtual address space.
struct vmspace {
    struct pmap pmap;
    struct vmap map;
};

struct vmspace* vmspace_new();

void vmspace_delete(struct vmspace* vm);
errno_t vmspace_unmap(struct vmspace* vm, uintptr_t vaddr, size_t size);
errno_t vmspace_protect(struct vmspace* vm, uintptr_t vaddr, size_t size, enum prot_flags prot);

bool vmspace_page_fault(struct vmspace* vm, uintptr_t vaddr, enum prot_flags prot);

errno_t vmspace_map(
    struct vmspace* vm,
    struct vmobject* vmobject,
    uintptr_t addr,
    size_t len,
    enum prot_flags prot,
    uintptr_t vmobject_offset
);

extern struct vmspace kernel_space;
