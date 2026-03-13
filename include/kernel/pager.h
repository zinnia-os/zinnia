#pragma once

#include <uapi/errno.h>
#include <stdint.h>

struct pager {
    errno_t (*try_get_page)(struct pager* pager, uintptr_t offset);
};
