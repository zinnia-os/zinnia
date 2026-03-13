#pragma once

#include <kernel/compiler.h>
#include <kernel/wait.h>
#include <uapi/errno.h>

// Initializes the global futex table.
void futex_init(void);

// Blocks the calling task if *addr == expected.
errno_t futex_wait(__user int* addr, int expected);

// Wakes up to `count` tasks waiting on the given address.
// Returns the number of tasks woken.
errno_t futex_wake(__user int* addr, int count);
