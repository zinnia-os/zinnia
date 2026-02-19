#include <kernel/cmdline.h>
#include <kernel/init.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

// Defined in linker script.
extern const struct cmdline_option* __ld_cmdline_start[];
extern const struct cmdline_option* __ld_cmdline_end[];

static char cmdline_buf[0x1000];

[[__init]]
void cmdline_parse(const char* cmdline) {
    const size_t len = strnlen(cmdline, sizeof(cmdline_buf) - 1);
    size_t idx = 0;
    memcpy(cmdline_buf, cmdline, len);

    while (1) {
        const struct cmdline_option** opt = __ld_cmdline_start;
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
        while (opt < __ld_cmdline_end) {
            const struct cmdline_option* current = *opt;
            if (!strcmp(current->name, name)) {
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
