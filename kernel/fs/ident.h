// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/types.h>
#include <lib/vec.h>

struct ident {
    uid_t uid, euid, suid;
    gid_t gid, egid, sgid;

    vec(gid_t) groups;
};

const struct ident* ident_kernel(void);

bool ident_in_group(const struct ident* identity, gid_t gid, bool use_real);

bool ident_is_root(const struct ident* identity, bool use_real);
