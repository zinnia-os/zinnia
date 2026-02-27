#ifndef ZINNIA_NAMESPACE_H
#define ZINNIA_NAMESPACE_H

#include <zinnia/handle.h>
#include <zinnia/status.h>

#ifdef __cplusplus
extern "C" {
#endif

// Creates a new namespace.
static inline zn_status_t zn_ns_create(zn_handle_t* out) {
    return ZN_ERR_UNSUPPORTED;
}

static inline zn_status_t zn_ns_move(zn_handle_t handle, zn_handle_t target_namespace) {
    return ZN_ERR_UNSUPPORTED;
}

#ifdef __cplusplus
}
#endif

#endif
