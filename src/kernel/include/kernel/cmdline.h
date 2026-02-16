#pragma once

#include <common/compiler.h>
#include <kernel/common.h>

#define CMDLINE_MAX 0x1000

#define CMDLINE_OPTION(opt_name, opt_func) \
    [[__used, __section(".cmdline")]] \
    static const struct cmdline_option UNIQUE_IDENT(cmdline_option) = { \
        .name = opt_name, \
        .func = opt_func, \
    }

struct cmdline_option {
    // The name of this option.
    const char* name;
    // Gets called if this option is present on the command line.
    // If an option is specified as `name=value`, then the `value` is passed as well.
    // Otherwise, it's NULL.
    void (*func)(const char* value);
};

// Parses the command line and invokes all options.
void cmdline_parse(const char* cmdline);
