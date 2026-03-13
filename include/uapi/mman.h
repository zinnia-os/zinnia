#pragma once

enum prot_flags {
    PROT_NONE = 0x00,
    PROT_READ = 0x01,
    PROT_WRITE = 0x02,
    PROT_EXEC = 0x04,
};

enum map_flags {
    MAP_FILE = 0x00,
    MAP_SHARED = 0x01,
    MAP_PRIVATE = 0x02,
    MAP_FIXED = 0x10,
    MAP_ANON = 0x20,
    MAP_ANONYMOUS = 0x20,
};

#define MAP_FAILED ((void*)(-1))
