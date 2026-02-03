#ifndef MENIX_RIGHTS_H
#define MENIX_RIGHTS_H

typedef enum {
    // Can be read from.
    MENIX_RIGHT_READ = 1u << 0,
    // Can be written to.
    MENIX_RIGHT_WRITE = 1u << 1,
    // Can be executed.
    MENIX_RIGHT_EXECUTE = 1u << 2,
    // Can be mapped in an address space.
    MENIX_RIGHT_MAP = 1u << 3,
    // Can be moved to another process.
    MENIX_RIGHT_MOVE = 1u << 4,
    // Can be cloned.
    MENIX_RIGHT_CLONE = 1u << 5,
    // Can be deleted.
    MENIX_RIGHT_DELETE = 1u << 6,

    // When cloning, use the same rights as the original.
    MENIX_RIGHTS_IDENTICAL = 1u << 31,
    MENIX_RIGHTS_COMMON = (MENIX_RIGHT_MOVE | MENIX_RIGHT_CLONE),
    MENIX_RIGHTS_RW = (MENIX_RIGHT_READ | MENIX_RIGHT_WRITE),
} menix_rights_t;

#endif
