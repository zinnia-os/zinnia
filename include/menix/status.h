#ifndef MENIX_STATUS_H
#define MENIX_STATUS_H

// Status values that may be returned by the kernel.
// A status value of 0 means everything is okay.
// A negative value indicates an error from the kernel.
// Positive values are available to user processes.
typedef enum {
    MENIX_OK = 0,
    // An internal error occured.
    MENIX_ERR_INTERNAL = -1,
    // Syscall number is not a recognized syscall.
    MENIX_ERR_BAD_SYSCALL = -2,
    // This operation is not supported.
    MENIX_ERR_UNSUPPORTED = -3,
    // System does not have enough free memory for this operation.
    MENIX_ERR_NO_MEMORY = -4,
    // Process can not own any more handles.
    MENIX_ERR_NO_HANDLES = -5,
    // One or more of the provided arguments is not valid.
    MENIX_ERR_BAD_ARG = -6,
    // Argument is outside of the range for valid values.
    MENIX_ERR_BAD_RANGE = -7,
    // Object handle does not name a valid object or correct type.
    MENIX_ERR_BAD_OBJECT = -8,
    // Object has insufficient permissions for this operation.
    MENIX_ERR_BAD_PERMS = -9,
    // Buffer is not large enough or doesn't point to a valid memory region.
    MENIX_ERR_BAD_BUFFER = -10,
} menix_status_t;

// Returns a string describing the error code.
const char* menix_status_to_string(menix_status_t status);

#endif
