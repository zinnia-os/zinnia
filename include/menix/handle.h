#ifndef MENIX_HANDLE_H
#define MENIX_HANDLE_H

#include <menix/rights.h>
#include <menix/status.h>
#include <stdint.h>

// A generic object handle.
typedef uint32_t menix_handle_t;

#define MENIX_HANDLE_INVALID      ((menix_handle_t)(0))
// Handle which always points to a channel connected to the init server.
#define MENIX_HANDLE_INIT_CHANNEL ((menix_handle_t)(-1))

// Checks an object handle for validity.
menix_status_t menix_handle_validate(menix_handle_t handle);

// Drops a an object handle.
// All further references using this handle are invalid. The numerical value
// may become valid again, but it is an error to keep using it.
// The underlying object gets freed only once no other handle references it.
menix_status_t menix_handle_drop(menix_handle_t handle);

// Creates a new handle pointing to the same object as pointed to
// by the original handle. The new handle may obtain new rights.
menix_status_t menix_handle_clone(menix_handle_t handle, menix_rights_t cloned_rights, menix_handle_t* cloned);

#endif
