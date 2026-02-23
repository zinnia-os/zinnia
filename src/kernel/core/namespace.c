#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <kernel/alloc.h>
#include <kernel/list.h>
#include <kernel/namespace.h>
#include <kernel/print.h>
#include <kernel/spin.h>
#include <stdatomic.h>

zn_status_t namespace_new(struct namespace** out) {
    if (!out)
        return ZN_ERR_INTERNAL;

    struct namespace* namespace = mem_alloc(sizeof(struct namespace), 0);
    if (!namespace)
        return ZN_ERR_NO_MEMORY;

    namespace->mutex = (struct mutex){0};
    namespace->next_handle = 1;

    *out = namespace;
    return ZN_OK;
}

zn_status_t namespace_add_desc(struct namespace* namespace, struct namespace_desc desc, zn_handle_t* out) {
    if (!namespace)
        return ZN_ERR_INTERNAL;

    struct namespace_desc* allocated_desc = mem_alloc(sizeof(*allocated_desc), 0);
    if (!allocated_desc)
        return ZN_ERR_NO_MEMORY;

    zn_handle_t handle = atomic_fetch_add_explicit(&namespace->next_handle, 1, memory_order_acq_rel);
    *allocated_desc = desc;
    allocated_desc->handle = handle;
    SLIST_INSERT_HEAD(&namespace->descriptors, allocated_desc, next);

    *out = handle;
    return ZN_OK;
}

zn_status_t namespace_get(struct namespace* namespace, zn_handle_t handle, struct namespace_desc* out) {
    if (!namespace)
        return ZN_ERR_INTERNAL;
    if (!out)
        return ZN_ERR_INTERNAL;

    if (handle == ZN_HANDLE_INVALID)
        return ZN_ERR_BAD_HANDLE;

    struct namespace_desc* iter;
    SLIST_FOREACH(iter, &namespace->descriptors, next) {
        if (iter->handle == handle)
            goto leave;
    }

    // Handle wasn't found.
    return ZN_ERR_BAD_HANDLE;

leave:
    *out = *iter;
    return ZN_OK;
}

zn_status_t namespace_del_desc(struct namespace* namespace, zn_handle_t handle) {
    if (!namespace)
        return ZN_ERR_INTERNAL;

    if (handle == ZN_HANDLE_INVALID)
        return ZN_ERR_BAD_HANDLE;

    struct namespace_desc* iter;
    SLIST_FOREACH(iter, &namespace->descriptors, next) {
        kprintf("iter: %lu\n", iter->handle);
        if (iter->handle == handle)
            goto leave;
    }

    // Handle wasn't found.
    return ZN_ERR_BAD_HANDLE;

leave:
    // TODO
    return ZN_ERR_UNSUPPORTED;
}
