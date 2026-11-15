// SPDX-License-Identifier: BSD-3-Clause

#include <lib/cmdline.h>
#include <lib/init.h>
#include <lib/stdlib.h>
#include <stddef.h>
#include <string.h>

// Defined in linker script.
extern const struct cmdline_option* __ld_cmdline_start[];
extern const struct cmdline_option* __ld_cmdline_end[];

static char cmdline_buf[0x1000];
static char cmdline_raw[0x1000];

const char* cmdline_get() {
    return cmdline_raw;
}

void cmdline_parse_buf(
    char* cmdline_buf,
    size_t len,
    const struct cmdline_option* const* start,
    const struct cmdline_option* const* end
) {
    size_t idx = 0;

    while (1) {
        const struct cmdline_option* const* opt = start;
        char* name = nullptr;
        char* value = nullptr;

        // Skip all leading spaces.
        while (idx < len && cmdline_buf[idx] == ' ')
            idx++;
        if (idx >= len)
            break;
        size_t name_idx = idx;
        name = cmdline_buf + name_idx;

        // Find the next equal sign or space.
        while (idx < len && cmdline_buf[idx] != '=' && cmdline_buf[idx] != ' ')
            idx++;
        if (idx > len)
            break;

        // Check if the option has a value (=foo).
        char seperator = cmdline_buf[idx];
        cmdline_buf[idx++] = 0;
        if (seperator == '=') {
            // Check if we need to escape the value.
            char check;
            if (cmdline_buf[idx] == '"') {
                check = '"';
                cmdline_buf[idx++] = 0;
            } else {
                check = ' ';
            }

            value = cmdline_buf + idx;

            // Skip the value.
            while (idx < len && cmdline_buf[idx] != check)
                idx++;
            if (idx > len)
                break;
            cmdline_buf[idx++] = 0;
        }

        // Find the corresponding option.
        while (opt < end) {
            const struct cmdline_option* current = *opt;
            if (!strcmp(current->name, name)) {
                if (value == nullptr)
                    break;
                switch (current->option_type) {
                case CMDLINE_STRING:
                    current->func_str(value);
                    break;
                case CMDLINE_BOOL:
                    if (!strcmp(value, "on") || !strcmp(value, "yes") || !strcmp(value, "true") || !strcmp(value, "1"))
                        current->func_bool(true);
                    else
                        current->func_bool(false);
                    break;
                case CMDLINE_INTPTR: {
                    int base = 10;
                    if (!strncmp(value, "0x", 2)) {
                        base = 0x10;
                        value += 2;
                    }
                    current->func_intptr(atol(value, base));
                    break;
                }
                case CMDLINE_UINTPTR: {
                    int base = 10;
                    if (!strncmp(value, "0x", 2)) {
                        base = 0x10;
                        value += 2;
                    }
                    current->func_uintptr(atolu(value, base));
                    break;
                }
                }
                break;
            }
            opt++;
        }

        if (idx >= len)
            break;
    }
}

[[__init]]
void cmdline_parse(const char* cmdline) {
    if (cmdline == nullptr)
        cmdline = "";

    const size_t len = strnlen(cmdline, sizeof(cmdline_buf) - 1);
    memcpy(cmdline_buf, cmdline, len);
    memcpy(cmdline_raw, cmdline, len);
    cmdline_parse_buf(cmdline_buf, len, __ld_cmdline_start, __ld_cmdline_end);
}
