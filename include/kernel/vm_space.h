#pragma once

#include <kernel/mutex.h>
#include <kernel/pmap.h>
#include <kernel/vm_object.h>
#include <uapi/errno.h>
#include <uapi/mman.h>

struct vmap {
    struct mutex mutex;
};

// Virtual address space.
struct vm_space {
    struct pmap pmap;
    struct vmap map;
};

struct vm_space* vm_space_new();

void vm_space_delete(struct vm_space* vm);
errno_t vm_space_unmap(struct vm_space* vm, uintptr_t vaddr, size_t size);
errno_t vm_space_protect(struct vm_space* vm, uintptr_t vaddr, size_t size, enum prot_flags prot);

bool vm_space_page_fault(struct vm_space* vm, uintptr_t vaddr, enum prot_flags prot);

errno_t vm_space_map(
    struct vm_space* vm,
    struct vm_object* vm_object,
    uintptr_t addr,
    size_t len,
    enum prot_flags prot,
    uintptr_t vm_object_offset
);

extern struct vm_space kernel_space;
