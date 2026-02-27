#ifndef ZINNIA_SYSTEM_H
#define ZINNIA_SYSTEM_H

#include <zinnia/status.h>
#include <zinnia/syscall_stubs.h>
#include <string.h>

#ifdef __cplusplus
extern "C" {
#endif

static inline void zn_log(const char* message, size_t len) {
    zn_syscall2(message, len, ZN_SYSCALL_LOG);
}

// Returns the page size of the system.
static inline size_t zn_page_size() {
    size_t size = 0;
    zn_syscall1(&size, ZN_SYSCALL_PAGE_SIZE);
    return size;
}

static inline zn_status_t zn_powerctl() {
    return ZN_ERR_UNSUPPORTED;
}

#ifdef __cplusplus
}
#endif

#endif
