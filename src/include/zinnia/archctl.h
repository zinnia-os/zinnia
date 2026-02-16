#ifndef ZINNIA_ARCHCTL_H
#define ZINNIA_ARCHCTL_H

#include <zinnia/status.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    // Does nothing.
    ZN_ARCHCTL_NONE = 0,
    // On x86_64, sets the FSBASE MSR to the value.
    ZN_ARCHCTL_SET_FSBASE = 1,
    // On x86_64, sets the GSBASE MSR to the value.
    ZN_ARCHCTL_SET_GSBASE = 2,
} zn_archctl_t;

// Performs an architecture-dependent operation identified by `op`.
zn_status_t zn_archctl(zn_archctl_t op, void* value);

#ifdef __cplusplus
}
#endif

#endif
