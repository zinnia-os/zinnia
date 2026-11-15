// SPDX-License-Identifier: BSD-3-Clause

#include <fs/ident.h>

static const struct ident kernel_identity = {0};

const struct ident* ident_kernel() {
    return &kernel_identity;
}

bool ident_in_group(const struct ident* identity, gid_t gid, bool use_real) {
    if (gid == (use_real ? identity->gid : identity->egid))
        return true;
    vec_foreach(&identity->groups, i) {
        if (vec_elem_get(&identity->groups, i) == gid)
            return true;
    }
    return false;
}

bool ident_is_root(const struct ident* identity, bool use_real) {
    return (use_real ? identity->uid : identity->euid) == 0;
}
