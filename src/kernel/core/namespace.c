#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <kernel/alloc.h>
#include <kernel/namespace.h>
#include <kernel/spin.h>

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

zn_status_t namespace_add_desc(struct namespace* namespace, struct namespace_desc* desc, zn_handle_t* out) {
    if (!namespace)
        return ZN_ERR_INTERNAL;

    return ZN_OK;
}

zn_status_t namespace_get(struct namespace* namespace, zn_handle_t handle, struct namespace_desc** desc) {
    if (!namespace)
        return ZN_ERR_INTERNAL;

    if (handle == ZN_HANDLE_INVALID)
        return ZN_ERR_BAD_HANDLE;

    return ZN_ERR_UNSUPPORTED;
}
