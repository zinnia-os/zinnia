#ifndef ZINNIA_HANDLE_H
#define ZINNIA_HANDLE_H

#include <zinnia/rights.h>
#include <zinnia/status.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// A generic object handle.
typedef int32_t zn_handle_t;

#define ZN_HANDLE_INVALID     ((zn_handle_t)(0))
#define ZN_HANDLE_THIS_NS     ((zn_handle_t)(-1))
#define ZN_HANDLE_THIS_VAS    ((zn_handle_t)(-2))
#define ZN_HANDLE_THIS_THREAD ((zn_handle_t)(-3))
#define ZN_HANDLE_ZERO_VMO    ((zn_handle_t)(-4))

// Checks an object handle for validity.
zn_status_t zn_handle_validate(zn_handle_t handle);

// Drops a an object handle.
// All further references using this handle are invalid. The numerical value
// may become valid again, but it is an error to keep using it.
// The underlying object gets freed only once no other handle references it.
zn_status_t zn_handle_drop(zn_handle_t handle);

// Creates a new handle pointing to the same object as pointed to
// by the original handle. The new handle may obtain new rights.
zn_status_t zn_handle_clone(zn_handle_t handle, zn_rights_t cloned_rights, zn_handle_t* cloned);

#ifdef __cplusplus
}
#endif

#endif
