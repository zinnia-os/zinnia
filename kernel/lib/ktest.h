// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <lib/utils.h>
#include <stddef.h> // IWYU pragma: keep
#include <stdint.h> // IWYU pragma: keep

struct ktest {
    const char* suite;
    const char* name;
    bool failed;
    bool skipped;
    const char* skip_reason;
};

struct ktest_case {
    const char* suite;
    const char* name;
    void (*func)(struct ktest* test);
};

#define KTEST(suite_name, case_name) \
    static void ktest_##suite_name##_##case_name(struct ktest* test); \
    [[__used, __section(".ktest"), gnu::aligned(8)]] \
    static const struct ktest_case ktest_##suite_name##_##case_name##_case = { \
        .suite = #suite_name, \
        .name = #case_name, \
        .func = ktest_##suite_name##_##case_name, \
    }; \
    static void ktest_##suite_name##_##case_name(struct ktest* test)

[[gnu::format(printf, 4, 5)]]
void ktest_fail(struct ktest* test, const char* file, int line, const char* fmt, ...);

void ktest_main();

#define _KTEST_CHECK(cond, on_fail, ...) \
    do { \
        if (!(cond)) { \
            ktest_fail(test, __FILE__, __LINE__, __VA_ARGS__); \
            on_fail; \
        } \
    } while (0)

#define _KTEST_CMP(a, op, b, on_fail) \
    do { \
        const typeof(true ? (a) : (b)) _ktest_a = (a); \
        const typeof(true ? (a) : (b)) _ktest_b = (b); \
        _KTEST_CHECK( \
            _ktest_a op _ktest_b, \
            on_fail, \
            "expected %s %s %s, got %#llx and %#llx", \
            #a, \
            #op, \
            #b, \
            (unsigned long long)_ktest_a, \
            (unsigned long long)_ktest_b \
        ); \
    } while (0)

#define _KTEST_STREQ(a, b, on_fail) \
    do { \
        const char* _ktest_a = (a); \
        const char* _ktest_b = (b); \
        _KTEST_CHECK( \
            _ktest_a && _ktest_b && !strcmp(_ktest_a, _ktest_b), \
            on_fail, \
            "expected %s == %s, got \"%s\" and \"%s\"", \
            #a, \
            #b, \
            _ktest_a ? _ktest_a : "(null)", \
            _ktest_b ? _ktest_b : "(null)" \
        ); \
    } while (0)

#define KTEST_EXPECT_TRUE(cond)    _KTEST_CHECK(cond, (void)0, "expected %s", #cond)
#define KTEST_EXPECT_FALSE(cond)   _KTEST_CHECK(!(cond), (void)0, "expected !(%s)", #cond)
#define KTEST_EXPECT_EQ(a, b)      _KTEST_CMP(a, ==, b, (void)0)
#define KTEST_EXPECT_NE(a, b)      _KTEST_CMP(a, !=, b, (void)0)
#define KTEST_EXPECT_LT(a, b)      _KTEST_CMP(a, <, b, (void)0)
#define KTEST_EXPECT_LE(a, b)      _KTEST_CMP(a, <=, b, (void)0)
#define KTEST_EXPECT_GT(a, b)      _KTEST_CMP(a, >, b, (void)0)
#define KTEST_EXPECT_GE(a, b)      _KTEST_CMP(a, >=, b, (void)0)
#define KTEST_EXPECT_STREQ(a, b)   _KTEST_STREQ(a, b, (void)0)
#define KTEST_EXPECT_NULL(ptr)     _KTEST_CHECK((ptr) == nullptr, (void)0, "expected %s to be null", #ptr)
#define KTEST_EXPECT_NOT_NULL(ptr) _KTEST_CHECK((ptr) != nullptr, (void)0, "expected %s to be non-null", #ptr)

#define KTEST_ASSERT_TRUE(cond)    _KTEST_CHECK(cond, return, "expected %s", #cond)
#define KTEST_ASSERT_FALSE(cond)   _KTEST_CHECK(!(cond), return, "expected !(%s)", #cond)
#define KTEST_ASSERT_EQ(a, b)      _KTEST_CMP(a, ==, b, return)
#define KTEST_ASSERT_NE(a, b)      _KTEST_CMP(a, !=, b, return)
#define KTEST_ASSERT_LT(a, b)      _KTEST_CMP(a, <, b, return)
#define KTEST_ASSERT_LE(a, b)      _KTEST_CMP(a, <=, b, return)
#define KTEST_ASSERT_GT(a, b)      _KTEST_CMP(a, >, b, return)
#define KTEST_ASSERT_GE(a, b)      _KTEST_CMP(a, >=, b, return)
#define KTEST_ASSERT_STREQ(a, b)   _KTEST_STREQ(a, b, return)
#define KTEST_ASSERT_NULL(ptr)     _KTEST_CHECK((ptr) == nullptr, return, "expected %s to be null", #ptr)
#define KTEST_ASSERT_NOT_NULL(ptr) _KTEST_CHECK((ptr) != nullptr, return, "expected %s to be non-null", #ptr)

#define KTEST_SKIP(reason) \
    do { \
        (test)->skipped = true; \
        (test)->skip_reason = (reason); \
        return; \
    } while (0)
