#pragma once

#include <uapi/archctl.h>
#include <uapi/errno.h>
#include <stdint.h>

errno_t arch_archctl(enum archctl_op op, uintptr_t value);
