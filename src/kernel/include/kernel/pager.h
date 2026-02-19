#pragma once

#include <zinnia/status.h>
#include <stdint.h>

struct pager {
    zn_status_t (*try_get_page)(struct pager* pager, uintptr_t offset);
};
