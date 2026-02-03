#ifndef MENIX_ARCHCTL_H
#define MENIX_ARCHCTL_H

#include <menix/status.h>
#include <stddef.h>

typedef enum {
    // Does nothing.
    MENIX_ARCHCTL_NONE = 0,
#ifdef __x86_64__
    // On x86_64, sets the FSBASE register to the value.
    MENIX_ARCHCTL_SET_FSBASE = 1,
#endif
} menix_archctl_t;

// Performs an architecture-dependent operation identified by `op`.
menix_status_t menix_archctl(menix_archctl_t op, size_t value);

#endif
