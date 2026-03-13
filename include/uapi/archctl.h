#pragma once

enum archctl_op {
    ARCHCTL_NONE = 0,
#ifdef __x86_64__
    ARCHCTL_SET_FSBASE = 1,
    ARCHCTL_SET_GSBASE = 2,
#endif
};
