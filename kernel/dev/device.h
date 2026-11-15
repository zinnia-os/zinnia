// SPDX-License-Identifier: BSD-3-Clause

#pragma once
#include <lib/module.h>

struct device {
    const char* name;
    struct module* owner;
};
