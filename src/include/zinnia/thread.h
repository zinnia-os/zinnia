#ifndef ZINNIA_THREAD_H
#define ZINNIA_THREAD_H

#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum zn_thread_flags {
    ZN_THREAD_NONE = 0,
};

// Creates a new thread in the given universe.
zn_status_t zn_thread_create(zn_handle_t universe, const char* name, size_t name_len, enum zn_thread_flags flags);

// Starts executing a thread.
zn_status_t zn_thread_start(zn_handle_t thread, uintptr_t ip, uintptr_t sp, uintptr_t arg);

// Starts executing a thread.
void zn_thread_exit(void);

#ifdef __cplusplus
}
#endif

#endif
