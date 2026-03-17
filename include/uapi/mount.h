#pragma once

enum mount_flags {
    MNT_RDONLY = 1 << 0,
    MNT_NOSUID = 1 << 1,
    MNT_NOEXEC = 1 << 2,
    MNT_RELATIME = 1 << 3,
    MNT_NOATIME = 1 << 4,
    MNT_REMOUNT = 1 << 5,
    MNT_FORCE = 1 << 6,
};
