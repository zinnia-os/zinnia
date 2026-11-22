// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stddef.h> // IWYU pragma: keep
#include <stdint.h> // IWYU pragma: keep

struct utest {
    const char* suite;
    const char* name;
    bool failed;
    bool skipped;
    const char* skip_reason;
};

struct utest_case {
    const char* suite;
    const char* name;
    const char* xfail_reason;
    void (*func)(struct utest* test);
};

#define _UTEST(suite_name, case_name, reason) \
    static void suite_name##_##case_name(struct utest* test); \
    [[gnu::used, gnu::retain, gnu::section("utest_cases"), gnu::aligned(8)]] \
    static const struct utest_case suite_name##_##case_name##_case = { \
        .suite = #suite_name, \
        .name = #case_name, \
        .xfail_reason = (reason), \
        .func = suite_name##_##case_name, \
    }; \
    static void suite_name##_##case_name(struct utest* test)

// Expected success
#define UTEST(suite_name, case_name)               _UTEST(suite_name, case_name, nullptr)
// Expected failure
#define UTEST_XFAIL(suite_name, case_name, reason) _UTEST(suite_name, case_name, reason)

[[gnu::format(printf, 1, 2)]]
void utest_log(const char* fmt, ...);

[[gnu::format(printf, 4, 5)]]
void utest_fail(struct utest* test, const char* file, int line, const char* fmt, ...);

void utest_run_all();

uintptr_t utest_getauxval(uintptr_t type);

bool utest_streq(const char* a, const char* b);

#define _UTEST_CHECK(cond, on_fail, ...) \
    do { \
        if (!(cond)) { \
            utest_fail(test, __FILE__, __LINE__, __VA_ARGS__); \
            on_fail; \
        } \
    } while (0)

#define _UTEST_CMP(a, op, b, on_fail) \
    do { \
        const typeof(true ? (a) : (b)) _utest_a = (a); \
        const typeof(true ? (a) : (b)) _utest_b = (b); \
        _UTEST_CHECK( \
            _utest_a op _utest_b, \
            on_fail, \
            "expected %s %s %s, got %#llx and %#llx", \
            #a, \
            #op, \
            #b, \
            (unsigned long long)_utest_a, \
            (unsigned long long)_utest_b \
        ); \
    } while (0)

#define EXPECT_TRUE(cond)  _UTEST_CHECK(cond, (void)0, "expected %s", #cond)
#define EXPECT_FALSE(cond) _UTEST_CHECK(!(cond), (void)0, "expected !(%s)", #cond)
#define EXPECT_EQ(a, b)    _UTEST_CMP(a, ==, b, (void)0)
#define EXPECT_NE(a, b)    _UTEST_CMP(a, !=, b, (void)0)
#define EXPECT_GT(a, b)    _UTEST_CMP(a, >, b, (void)0)
#define EXPECT_GE(a, b)    _UTEST_CMP(a, >=, b, (void)0)
#define EXPECT_STREQ(a, b) _UTEST_CHECK(utest_streq((a), (b)), (void)0, "expected %s == \"%s\"", #a, (b))

#define ASSERT_TRUE(cond) _UTEST_CHECK(cond, return, "expected %s", #cond)
#define ASSERT_EQ(a, b)   _UTEST_CMP(a, ==, b, return)
#define ASSERT_NE(a, b)   _UTEST_CMP(a, !=, b, return)

#define SKIP(reason) \
    do { \
        test->skipped = true; \
        test->skip_reason = (reason); \
        return; \
    } while (0)
