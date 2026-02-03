#include <menix/status.h>

const char* menix_status_to_string(menix_status_t err) {
    switch (err) {
    case MENIX_OK:
        return "No error (OK)";
    case MENIX_ERR_INTERNAL:
        return "An internal error occured (INTERNAL)";
    case MENIX_ERR_BAD_SYSCALL:
        return "Syscall number is not a recognized syscall (BAD_SYSCALL)";
    case MENIX_ERR_UNSUPPORTED:
        return "This operation is not supported (UNSUPPORTED)";
    case MENIX_ERR_NO_MEMORY:
        return "System does not have enough free memory for this operation (NO_MEMORY)";
    case MENIX_ERR_NO_HANDLES:
        return "Process can not own any more handles (NO_HANDLES)";
    case MENIX_ERR_BAD_ARG:
        return "One or more of the provided arguments is not valid (BAD_ARG)";
    case MENIX_ERR_BAD_RANGE:
        return "Argument is outside of the range for valid values (BAD_RANGE)";
    case MENIX_ERR_BAD_OBJECT:
        return "Object handle does not name a valid object (BAD_OBJECT)";
    case MENIX_ERR_BAD_PERMS:
        return "Object has insufficient permissions for this operation (BAD_PERMS)";
    case MENIX_ERR_BAD_BUFFER:
        return "Buffer is not large enough or doesn't point to a valid memory region (BAD_BUFFER)";
    }

    return "Unknown error code";
}
