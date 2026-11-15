// SPDX-License-Identifier: BSD-3-Clause

#define _GNU_SOURCE

#include <errno.h>
#include <fcntl.h>
#include <getopt.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

constexpr size_t LINE_MAX_LEN = 8192;

static const char* const level_names[] = {
    "emerg",
    "alert",
    "crit",
    "err",
    "warn",
    "notice",
    "info",
    "debug",
};

#define COLOR_RESET  "\033[0m"
#define COLOR_TIME   "\033[32m"
#define COLOR_SUBSYS "\033[33m"

static const char* const level_colors[] = {
    "\033[7;31m",
    "\033[7;31m",
    "\033[1;31m",
    "\033[31m",
    "\033[1m",
    "",
    "",
    "",
};

struct options {
    bool follow;
    bool raw;
    bool decode;
    bool color;
    unsigned int level_mask;
};

static int hex_value(char c) {
    if (c >= '0' && c <= '9')
        return c - '0';
    if (c >= 'a' && c <= 'f')
        return c - 'a' + 10;
    if (c >= 'A' && c <= 'F')
        return c - 'A' + 10;
    return -1;
}

static size_t unescape(const char* in, size_t len, char* out) {
    size_t n = 0;
    for (size_t i = 0; i < len; i++) {
        if (in[i] == '\\' && i + 3 < len && in[i + 1] == 'x') {
            const int hi = hex_value(in[i + 2]);
            const int lo = hex_value(in[i + 3]);
            if (hi >= 0 && lo >= 0) {
                out[n++] = (char)(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        out[n++] = in[i];
    }
    return n;
}

static const char* find_caller(char* header) {
    char* save;
    for (char* field = strtok_r(header, ",", &save); field; field = strtok_r(nullptr, ",", &save)) {
        if (!strncmp(field, "caller=", 7))
            return field + 7;
    }
    return nullptr;
}

static size_t subsystem_len(const char* text, size_t len) {
    for (size_t i = 0; i + 1 < len; i++) {
        if (text[i] == ':' && (text[i + 1] == ' ' || text[i + 1] == '\t'))
            return i + 1;
    }
    return 0;
}

static void print_record(const struct options* opts, char* record, size_t len) {
    if (opts->raw) {
        printf("%.*s\n", (int)len, record);
        return;
    }

    char* msg = memchr(record, ';', len);
    if (!msg)
        return;
    *msg++ = '\0';

    unsigned int prio;
    uint64_t seq;
    uint64_t usec;
    if (sscanf(record, "%u,%" SCNu64 ",%" SCNu64, &prio, &seq, &usec) != 3)
        return;

    const unsigned int level = prio & 7;
    if (!(opts->level_mask & (1u << level)))
        return;

    const char* caller = find_caller(record);

    static char text[LINE_MAX_LEN];
    const size_t text_len = unescape(msg, len - (size_t)(msg - record), text);

    if (opts->decode)
        printf("%-6s: ", level_names[level]);

    if (opts->color)
        printf(COLOR_TIME);
    printf("[%5" PRIu64 ".%06" PRIu64 "]", usec / 1'000'000, usec % 1'000'000);
    if (opts->color)
        printf(COLOR_RESET);
    putchar(' ');

    if (caller)
        printf("[%6s] ", caller);

    const char* text_pos = text;
    size_t remaining = text_len;
    const size_t subsys = subsystem_len(text, text_len);
    if (subsys) {
        if (opts->color)
            printf(COLOR_SUBSYS "%.*s" COLOR_RESET, (int)subsys, text);
        else
            printf("%.*s", (int)subsys, text);
        text_pos += subsys;
        remaining -= subsys;
    }

    const char* color = opts->color ? level_colors[level] : "";
    if (*color)
        printf("%s%.*s" COLOR_RESET "\n", color, (int)remaining, text_pos);
    else
        printf("%.*s\n", (int)remaining, text_pos);
}

static int parse_levels(const char* list, unsigned int* mask) {
    char* copy = strdup(list);
    if (!copy)
        return -1;

    *mask = 0;
    char* save;
    for (char* tok = strtok_r(copy, ",", &save); tok; tok = strtok_r(nullptr, ",", &save)) {
        bool found = false;
        for (unsigned int i = 0; i < 8; i++) {
            if (!strcmp(tok, level_names[i])) {
                *mask |= 1u << i;
                found = true;
                break;
            }
        }
        if (!found) {
            fprintf(stderr, "kmsg: unknown level '%s'\n", tok);
            free(copy);
            return -1;
        }
    }
    free(copy);
    return 0;
}

static void usage(const char* prog) {
    fprintf(
        stderr,
        "usage: %s [-w] [-r] [-x] [-l level[,level...]] [--color=auto|always|never]\n"
        "  -w, --follow     wait for new messages\n"
        "  -r, --raw        print raw /dev/kmsg records\n"
        "  -x, --decode     show the log level of each message\n"
        "  -l, --level      only show the given levels (emerg,alert,crit,err,warn,notice,info,debug)\n"
        "  -L, --color      colorize output (auto, always, never)\n",
        prog
    );
}

int main(int argc, char** argv) {
    struct options opts = {
        .level_mask = 0xFF,
    };
    int color_mode = -1;

    static const struct option long_opts[] = {
        {"follow", no_argument, nullptr, 'w'},
        {"raw", no_argument, nullptr, 'r'},
        {"decode", no_argument, nullptr, 'x'},
        {"level", required_argument, nullptr, 'l'},
        {"color", optional_argument, nullptr, 'L'},
        {"help", no_argument, nullptr, 'h'},
        {},
    };

    int c;
    while ((c = getopt_long(argc, argv, "wrxl:L::h", long_opts, nullptr)) != -1) {
        switch (c) {
        case 'w':
            opts.follow = true;
            break;
        case 'r':
            opts.raw = true;
            break;
        case 'x':
            opts.decode = true;
            break;
        case 'l':
            if (parse_levels(optarg, &opts.level_mask))
                return 1;
            break;
        case 'L':
            if (!optarg || !strcmp(optarg, "always"))
                color_mode = 1;
            else if (!strcmp(optarg, "never"))
                color_mode = 0;
            else if (!strcmp(optarg, "auto"))
                color_mode = -1;
            else {
                usage(argv[0]);
                return 1;
            }
            break;
        case 'h':
            usage(argv[0]);
            return 0;
        default:
            usage(argv[0]);
            return 1;
        }
    }

    opts.color = color_mode < 0 ? isatty(STDOUT_FILENO) : color_mode;

    const int fd = open("/dev/kmsg", O_RDONLY | (opts.follow ? 0 : O_NONBLOCK));
    if (fd < 0) {
        perror("kmsg: /dev/kmsg");
        return 1;
    }

    static char record[LINE_MAX_LEN];
    while (true) {
        const ssize_t n = read(fd, record, sizeof(record) - 1);
        if (n < 0) {
            if (errno == EINTR)
                continue;
            if (errno == EPIPE)
                continue;
            if (errno == EAGAIN)
                break;
            perror("kmsg: read");
            close(fd);
            return 1;
        }
        if (n == 0)
            break;
        record[n] = '\0';
        for (char* line = record; line < record + n;) {
            char* end = memchr(line, '\n', (size_t)(record + n - line));
            if (!end)
                end = record + n;
            *end = '\0';
            if (*line != ' ')
                print_record(&opts, line, (size_t)(end - line));
            line = end + 1;
        }
        if (opts.follow)
            fflush(stdout);
    }

    close(fd);
    return 0;
}
