#pragma once

#include <uapi/types.h>

struct identity {
    uid_t uid, euid, suid;
    gid_t gid, egid, sgid;
};

extern struct identity kernel_identity;
