#include <zinnia/status.h>
#include <kernel/mem.h>
#include <kernel/namespace.h>
#include <kernel/spin.h>

zn_status_t namespace_new(struct namespace** out) {
    if (!out)
        return ZN_ERR_INVALID;

    struct namespace* namespace = mem_alloc(sizeof(struct namespace), 0);
    if (!namespace)
        return ZN_ERR_NO_MEMORY;

    namespace->mutex = (struct mutex){0};
    namespace->next_handle = 1;

    *out = namespace;
    return ZN_OK;
}
