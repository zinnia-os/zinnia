#include <zinnia/handle.h>
#include <zinnia/namespace.h>
#include <zinnia/status.h>
#include <vdso/common.h>

VDSO_FUNC(zn_status_t, zn_ns_create, zn_handle_t* out) {
    return ZN_ERR_UNSUPPORTED;
}

VDSO_FUNC(zn_status_t, zn_ns_move, zn_handle_t handle, zn_handle_t target_namespace) {
    return ZN_ERR_UNSUPPORTED;
}
