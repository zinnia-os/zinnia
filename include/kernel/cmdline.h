#pragma once

#include <kernel/compiler.h>
#include <kernel/utils.h>
#include <stdint.h>

#define _CMDLINE_OPTION(x, opt_name, opt_func) \
    static const struct cmdline_option x = { \
        .name = opt_name, \
        .option_type = _Generic( \
            (opt_func), \
            void (*)(const char*): CMDLINE_STRING, \
            void (*)(bool): CMDLINE_BOOL, \
            void (*)(intptr_t): CMDLINE_INTPTR, \
            void (*)(uintptr_t): CMDLINE_UINTPTR \
        ), \
        .func = (void*)(opt_func), \
    }; \
    [[__used, __section(".cmdline")]] \
    static const struct cmdline_option* CONCAT(x, _ptr) = &x;

#define CMDLINE_OPTION(opt_name, opt_func) _CMDLINE_OPTION(UNIQUE_IDENT(cmdline_option), opt_name, opt_func)

enum cmdline_type {
    CMDLINE_STRING,
    CMDLINE_BOOL,
    CMDLINE_INTPTR,
    CMDLINE_UINTPTR,
};

struct cmdline_option {
    const char* name;
    // Gets called if this option is present on the command line.
    union {
        void (*func_str)(const char* value);
        void (*func_bool)(bool value);
        void (*func_intptr)(intptr_t value);
        void (*func_uintptr)(uintptr_t value);
        void* func;
    };
    enum cmdline_type option_type;
};

// Parses the command line and invokes all options.
void cmdline_parse(const char* cmdline);
