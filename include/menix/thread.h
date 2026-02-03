#ifndef MENIX_THREAD_H
#define MENIX_THREAD_H

#include <menix/handle.h>
#include <menix/status.h>

enum menix_thread_flags {
    MENIX_THREAD_
};

// Creates a new thread.
menix_status_t menix_thread_create(const char* name, menix_handle_t address_space, enum menix_thread_flags flags);

#endif
