// SPDX-License-Identifier: BSD-3-Clause

#include <arch/archctl.h>
#include <fs/ident.h>
#include <lib/print.h>
#include <lib/spin.h>
#include <mem/alloc.h>
#include <mem/usercopy.h>
#include <syscalls/private.h>
#include <syscalls/syscall.h>
#include <config.h>
#include <stdint.h>
#include <string.h>
#include <zinnia/utsname.h>

enum syslog_kind : uintptr_t {
    SYSLOG_EMERG = 0,
    SYSLOG_ALERT = 1,
    SYSLOG_CRIT = 2,
    SYSLOG_ERR = 3,
    SYSLOG_WARNING = 4,
    SYSLOG_NOTICE = 5,
    SYSLOG_INFO = 6,
    SYSLOG_DEBUG = 7,
};

static struct spinlock uname_lock = {};

static struct __utsname uname_buf = {
    .sysname = "Zinnia",
    .release = ZINNIA_VERSION,
    .version = CONFIG_KERNEL_FLAVOR,
    .nodename = "localhost",
    .machine = ZINNIA_ARCH,
    .domainname = "(none)",
};

struct syscall_ret sys_archctl(uintptr_t op, uintptr_t arg) {
    return arch_archctl(op, arg);
}

struct syscall_ret sys_getuname(__user struct __utsname* ptr) {
    struct syscall_ret ret = {};

    if (!ptr) {
        ret.error = EINVAL;
        return ret;
    }

    spin_lock(&uname_lock);
    struct __utsname write_buf = uname_buf;
    spin_unlock(&uname_lock);

    if (!usercopy_write(ptr, &write_buf, sizeof(write_buf)))
        ret.error = EFAULT;

    return ret;
}

struct syscall_ret sys_setuname(const __user struct __utsname* ptr) {
    struct syscall_ret ret = {};

    if (!ptr) {
        ret.error = EINVAL;
        return ret;
    }

    if (!ident_is_root(&self()->identity, false)) {
        ret.error = EPERM;
        return ret;
    }

    struct __utsname read_buf;
    if (!usercopy_read(&read_buf, ptr, sizeof(read_buf))) {
        ret.error = EFAULT;
        return ret;
    }

    spin_lock(&uname_lock);

    strncpy(uname_buf.sysname, read_buf.sysname, sizeof(uname_buf.sysname) - 1);
    strncpy(uname_buf.release, read_buf.release, sizeof(uname_buf.release) - 1);
    strncpy(uname_buf.version, read_buf.version, sizeof(uname_buf.version) - 1);
    strncpy(uname_buf.nodename, read_buf.nodename, sizeof(uname_buf.nodename) - 1);
    strncpy(uname_buf.machine, read_buf.machine, sizeof(uname_buf.machine) - 1);
    strncpy(uname_buf.domainname, read_buf.domainname, sizeof(uname_buf.domainname) - 1);

    spin_unlock(&uname_lock);

    return ret;
}

struct syscall_ret sys_syslog(enum syslog_kind kind, __user char* message, size_t len) {
    struct syscall_ret ret = {};

    const size_t buf_len = MIN(len, 255) + 1;
    char* buf = mem_alloc(buf_len, ALLOC_NOZERO);
    if (!buf) {
        ret.error = ENOMEM;
        return ret;
    }
    if (!usercopy_read(buf, message, buf_len - 1)) {
        mem_free(buf, buf_len);

        ret.error = EFAULT;
        return ret;
    }
    buf[buf_len - 1] = 0;

    const char* log_level;
    switch (kind) {
    case SYSLOG_EMERG:
        log_level = "emerg";
        break;
    case SYSLOG_ALERT:
        log_level = "alert";
        break;
    case SYSLOG_CRIT:
        log_level = "crit";
        break;
    case SYSLOG_ERR:
        log_level = "err";
        break;
    case SYSLOG_WARNING:
        log_level = "warning";
        break;
    case SYSLOG_NOTICE:
        log_level = "notice";
        break;
    case SYSLOG_INFO:
        log_level = "info";
        break;
    case SYSLOG_DEBUG:
        log_level = "debug";
        break;
    default:
        log_level = "invalid";
    }

    log("syslog(%s): %s\n", log_level, buf);

    mem_free(buf, buf_len);

    return ret;
}
