#pragma once

#include <kernel/list.h>
#include <kernel/spin.h>
#include <kernel/virt.h>
#include <uapi/errno.h>
#include <stddef.h>
#include <stdint.h>

struct vm_object {
    size_t refcount;
    errno_t (*get_page)(struct vm_object* vm_object, uintptr_t offset_idx, struct page** out);
};

errno_t vm_object_read(struct vm_object* obj, uintptr_t offset, void* buf, size_t len, size_t* actual_read);
errno_t vm_object_write(struct vm_object* obj, uintptr_t offset, const void* buf, size_t len, size_t* actual_written);
errno_t vm_object_copy(
    struct vm_object* target,
    uintptr_t target_offset,
    struct vm_object* src,
    uintptr_t src_offset,
    size_t len,
    size_t* actual_copied
);

struct pager_ops {
    errno_t (*get_page)(struct pager_ops* pager, uintptr_t offset_idx, struct page** out);
    errno_t (*put_page)(struct pager_ops* pager, uintptr_t offset_idx, struct page* page);
};

struct page_list {
    uintptr_t offset;
    struct page* value;
    SLIST_LINK(struct page_list*) next;
};

struct paged_vmo {
    struct vm_object object;
    struct pager_ops source;
    SLIST_HEAD(struct page_list*) cache;
    struct spinlock lock;
};

errno_t vm_object_new_paged(struct pager_ops ops, struct paged_vmo** out);
errno_t vm_object_new_phys(struct paged_vmo** out);

// Creates a VMO backed by a specific physical address range (e.g. for MMIO).
errno_t vm_object_new_phys_range(uintptr_t addr, size_t length, struct vm_object** out);
