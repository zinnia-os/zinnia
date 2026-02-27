#ifndef ZINNIA_RANDOM_H
#define ZINNIA_RANDOM_H

#include <zinnia/status.h>
#include <zinnia/syscall_stubs.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

enum {
    ZN_RANDOM_MAX_ENTROPY = 0x1000
};

// Stores random bytes in a buffer.
static inline zn_status_t zn_random_get(void* addr, size_t len) {
    return zn_syscall2(addr, len, ZN_SYSCALL_RANDOM_GET);
}

// Adds entropy to the RNG.
static inline zn_status_t zn_random_entropy(const void* addr, size_t len) {
    return zn_syscall2(addr, len, ZN_SYSCALL_RANDOM_ENTROPY);
}

#ifdef __cplusplus
}
#endif

#endif
