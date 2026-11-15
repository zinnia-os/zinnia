// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>
#include <zinnia/mman.h>

enum prot_flags : uint8_t {
    PROT_NONE = __PROT_NONE,
    PROT_READ = __PROT_READ,
    PROT_WRITE = __PROT_WRITE,
    PROT_EXEC = __PROT_EXEC,
};

enum map_flags : uint8_t {
    MAP_FILE = __MAP_FILE,
    MAP_SHARED = __MAP_SHARED,
    MAP_PRIVATE = __MAP_PRIVATE,
    MAP_FIXED = __MAP_FIXED,
    MAP_ANON = __MAP_ANON,
};

enum ms_flags : uint8_t {
    MS_ASYNC = __MS_ASYNC,
    MS_INVALIDATE = __MS_INVALIDATE,
    MS_SYNC = __MS_SYNC,
};
