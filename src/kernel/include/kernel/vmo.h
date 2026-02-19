#pragma once

#include <zinnia/status.h>
#include <kernel/list.h>
#include <kernel/spin.h>
#include <kernel/virt.h>
#include <stddef.h>
#include <stdint.h>

struct vmo {
    zn_status_t (*get_page)(struct vmo* vmo, uintptr_t offset_idx, struct page** out);
};

zn_status_t vmo_read(struct vmo* obj, uintptr_t offset, void* buf, size_t len, size_t* actual_read);
zn_status_t vmo_write(struct vmo* obj, uintptr_t offset, const void* buf, size_t len, size_t* actual_written);
zn_status_t vmo_copy(
    struct vmo* target,
    uintptr_t target_offset,
    struct vmo* src,
    uintptr_t src_offset,
    size_t len,
    size_t* actual_copied
);

struct pager_ops {
    zn_status_t (*get_page)(struct pager_ops* pager, uintptr_t offset_idx, struct page** out);
    zn_status_t (*put_page)(struct pager_ops* pager, uintptr_t offset_idx, struct page* page);
};

struct page_list {
    uintptr_t offset;
    struct page* value;
    SLIST_LINK(struct page_list*) next;
};

struct paged_vmo {
    struct vmo object;
    struct pager_ops source;
    SLIST_HEAD(struct page_list*) cache;
    struct spinlock lock;
};

zn_status_t vmo_new_paged(struct pager_ops ops, struct paged_vmo** out);
zn_status_t vmo_new_phys(struct paged_vmo** out);
