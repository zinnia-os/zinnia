// SPDX-License-Identifier: BSD-3-Clause

#include <string.h>
#include <tests/user/lib/syscall.h>
#include <tests/user/lib/utest.h>
#include <zinnia/errno.h>
#include <zinnia/utsname.h>

UTEST(syslog, writes_message) {
    const char msg[] = "utest: syslog test";
    struct sys_ret ret = sys_syslog(SYSLOG_INFO, msg, sizeof(msg) - 1);
    EXPECT_EQ(ret.error, 0);
}

UTEST(syslog, rejects_bad_pointer) {
    struct sys_ret ret = sys_syslog(SYSLOG_INFO, (const char*)0x10, 8);
    EXPECT_EQ(ret.error, __EFAULT);
}

UTEST(system, uname) {
    struct __utsname uts = {};
    struct sys_ret ret = sys_getuname(&uts);
    ASSERT_EQ(ret.error, 0);
    EXPECT_STREQ(uts.sysname, "Zinnia");
    EXPECT_STREQ(uts.machine, "x86_64");
}

UTEST(syslog, long_message) {
    char msg[1024];
    memset(msg, 'x', sizeof(msg));
    EXPECT_EQ(sys_syslog(SYSLOG_INFO, msg, sizeof(msg)).error, 0);
}

UTEST(system, uname_rejects_null) {
    EXPECT_EQ(sys_getuname(nullptr).error, __EINVAL);
    EXPECT_EQ(sys_setuname(nullptr).error, __EINVAL);
}

UTEST(system, uname_rejects_bad_pointer) {
    EXPECT_EQ(sys_getuname((void*)0x10).error, __EFAULT);
    EXPECT_EQ(sys_setuname((const void*)0x10).error, __EFAULT);
}

UTEST(system, setuname) {
    struct __utsname original = {};
    ASSERT_EQ(sys_getuname(&original).error, 0);

    struct __utsname changed = original;
    memset(changed.nodename, 0, sizeof(changed.nodename));
    memcpy(changed.nodename, "foobar", sizeof("foobar"));
    ASSERT_EQ(sys_setuname(&changed).error, 0);

    struct __utsname now = {};
    EXPECT_EQ(sys_getuname(&now).error, 0);
    EXPECT_STREQ(now.nodename, "foobar");
    EXPECT_STREQ(now.sysname, original.sysname);

    EXPECT_EQ(sys_setuname(&original).error, 0);
    EXPECT_EQ(sys_getuname(&now).error, 0);
    EXPECT_STREQ(now.nodename, original.nodename);
}
