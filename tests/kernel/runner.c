// SPDX-License-Identifier: BSD-3-Clause

#include <lib/cmdline.h>
#include <lib/ktest.h>
#include <lib/print.h>
#include <stdarg.h>

extern const struct ktest_case __ld_ktest_start[];
extern const struct ktest_case __ld_ktest_end[];

static bool ktest_enabled = false;

static void ktest_option(bool value) {
    ktest_enabled = value;
}
CMDLINE_OPTION("ktest", "Run all kernel tests", ktest_option);

void ktest_fail(struct ktest* test, const char* file, int line, const char* fmt, ...) {
    char buf[256];
    va_list args;
    va_start(args, fmt);
    kvsnprintf(buf, sizeof(buf), fmt, args);
    va_end(args);

    test->failed = true;
    log("# %s.%s: %s:%d: %s\n", test->suite, test->name, file, line, buf);
}

void ktest_main() {
    if (!ktest_enabled)
        return;

    size_t index = 0;
    for (const struct ktest_case* c = __ld_ktest_start; c < __ld_ktest_end; c++) {
        struct ktest test = {.suite = c->suite, .name = c->name};
        c->func(&test);

        index++;
        if (test.failed)
            log("not ok %zu %s.%s\n", index, c->suite, c->name);
        else if (test.skipped)
            log("ok %zu %s.%s # SKIP %s\n", index, c->suite, c->name, test.skip_reason ? test.skip_reason : "");
        else
            log("ok %zu %s.%s\n", index, c->suite, c->name);
    }

    log("1..%zu\n", index);
}
