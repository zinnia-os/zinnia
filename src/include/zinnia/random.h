#ifndef ZINNIA_RANDOM_H
#define ZINNIA_RANDOM_H

#include <zinnia/status.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

enum {
    ZN_RANDOM_MAX_ENTROPY = 0x1000
};

// Stores random bytes in a buffer.
zn_status_t zn_random_get(void* addr, size_t len);

// Adds entropy to the RNG.
zn_status_t zn_random_entropy(const void* addr, size_t len);

#ifdef __cplusplus
}
#endif

#endif
