#ifndef ZINNIA_THREAD_H
#define ZINNIA_THREAD_H

#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <zinnia/syscall_stubs.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum zn_thread_flags {
    ZN_THREAD_NONE = 0,
};

// Creates a new thread in the given universe.
static inline zn_status_t zn_thread_create(
    zn_handle_t universe,
    const char* name,
    size_t name_len,
    enum zn_thread_flags flags
) {
    return zn_syscall4(universe, name, name_len, flags, ZN_SYSCALL_THREAD_CREATE);
}

// Starts executing a thread.
static inline zn_status_t zn_thread_start(zn_handle_t thread, uintptr_t ip, uintptr_t sp, uintptr_t arg) {
    return zn_syscall4(thread, ip, sp, arg, ZN_SYSCALL_THREAD_START);
}

// Stops execution of the calling thread.
static inline void zn_thread_exit(void) {
    zn_syscall0(ZN_SYSCALL_THREAD_EXIT);
}

#ifdef __cplusplus
}
#endif

#endif
