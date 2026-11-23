// SPDX-License-Identifier: BSD-3-Clause

#include <dev/block/blockdev.h>
#include <fs/ident.h>
#include <lib/atomic.h>
#include <lib/clock.h>
#include <lib/futex.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/process.h>
#include <lib/random.h>
#include <lib/sched.h>
#include <lib/signal.h>
#include <lib/spin.h>
#include <lib/time.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <mem/usercopy.h>
#include <syscalls/private.h>
#include <syscalls/syscall.h>
#include <bits/archctl.h>
#include <config.h>
#include <stdckdint.h>
#include <stdint.h>
#include <string.h>
#include <zinnia/limits.h>
#include <zinnia/reboot.h>
#include <zinnia/resource.h>
#include <zinnia/time.h>
#include <zinnia/unistd.h>
#include <zinnia/utsname.h>
#if CONFIG_ACPI
#include <dev/acpi/acpi.h>
#endif

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

static struct process_rlimit* rlimit_of(struct process* proc, int resource) {
    switch (resource) {
    case __RLIMIT_NOFILE:
        return &proc->limit_nofile;
    case __RLIMIT_CORE:
        return &proc->limit_core;
    default:
        return nullptr;
    }
}

constexpr size_t ENTROPY_MAX = 256;

static errno_t read_timespec_ns(const __user struct __timespec* user, uint64_t* out) {
    struct __timespec ts;
    if (!usercopy_read(&ts, user, sizeof(ts)))
        return EFAULT;
    return time_to_nano(&ts, out);
}

static uint64_t deadline_after(uint64_t ns) {
    uint64_t deadline;
    if (ckd_add(&deadline, clock_get_elapsed_ns(), ns))
        return UINT64_MAX;
    return deadline;
}

static bool clock_valid(uintptr_t clock) {
    return clock <= __CLOCK_BOOTTIME;
}

bool clock_is_realtime(uintptr_t clock) {
    return clock == __CLOCK_REALTIME || clock == __CLOCK_REALTIME_COARSE;
}

static struct __itimerval current_itimer(struct process* proc) {
    uint64_t interval;
    const uint64_t value = signal_itimer_get(proc, &interval);
    return (struct __itimerval){
        .it_interval = time_tv_from_nano(interval),
        .it_value = time_tv_from_nano(ROUND_UP(value, 1'000)),
    };
}

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

struct syscall_ret sys_clock_get(uintptr_t clock, __user struct __timespec* tp) {
    struct syscall_ret ret = {};

    if (!clock_valid(clock)) {
        ret.error = EINVAL;
        return ret;
    }

    const uint64_t ns = clock_is_realtime(clock) ? clock_get_realtime_ns() : clock_get_elapsed_ns();
    const struct __timespec ts = time_from_nano(ns);
    if (!usercopy_write(tp, &ts, sizeof(ts)))
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_clock_set(uintptr_t clock, const __user struct __timespec* tp) {
    struct syscall_ret ret = {};

    if (clock != __CLOCK_REALTIME) {
        ret.error = EINVAL;
        return ret;
    }
    if (!ident_is_root(&self()->identity, false)) {
        ret.error = EPERM;
        return ret;
    }

    uint64_t ns;
    ret.error = read_timespec_ns(tp, &ns);
    if (ret.error)
        return ret;
    if (ns > INT64_MAX) {
        ret.error = EINVAL;
        return ret;
    }

    clock_set_realtime_ns(ns);
    return ret;
}

struct syscall_ret sys_clock_getres(uintptr_t clock, __user struct __timespec* tp) {
    struct syscall_ret ret = {};

    if (!clock_valid(clock)) {
        ret.error = EINVAL;
        return ret;
    }
    if (!tp)
        return ret;
    const struct __timespec ts = {.tv_sec = 0, .tv_nsec = 1};
    if (!usercopy_write(tp, &ts, sizeof(ts)))
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_getentropy(__user void* buffer, size_t length) {
    struct syscall_ret ret = {};

    if (length > ENTROPY_MAX) {
        ret.error = EIO;
        return ret;
    }

    uint8_t data[ENTROPY_MAX];
    random_fill(data, length);
    const bool ok = usercopy_write(buffer, data, length);
    memset(data, 0, sizeof(data));
    if (!ok)
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_futex_wait(__user int* pointer, int expected, const __user struct __timespec* timeout) {
    struct syscall_ret ret = {};

    uint64_t deadline = 0;
    if (timeout) {
        uint64_t ns;
        ret.error = read_timespec_ns(timeout, &ns);
        if (ret.error)
            return ret;
        deadline = deadline_after(ns);
    }

    struct task* task = percpu_get()->sched.current;
    ret.error = futex_wait(task->space, (uintptr_t)pointer, expected, timeout ? &deadline : nullptr);
    return ret;
}

struct syscall_ret sys_futex_wake(__user int* pointer, uint32_t count) {
    struct syscall_ret ret = {};

    if ((uintptr_t)pointer % sizeof(int)) {
        ret.error = EINVAL;
        return ret;
    }
    struct task* task = percpu_get()->sched.current;
    ret.value = futex_wake(task->space, (uintptr_t)pointer, count);
    return ret;
}

struct syscall_ret sys_itimer_get(int which, __user struct __itimerval* curr) {
    struct syscall_ret ret = {};

    if (which != __ITIMER_REAL) {
        ret.error = EINVAL;
        return ret;
    }
    const struct __itimerval value = current_itimer(self());
    if (!usercopy_write(curr, &value, sizeof(value)))
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_itimer_set(
    int which,
    const __user struct __itimerval* new_value,
    __user struct __itimerval* old
) {
    struct syscall_ret ret = {};

    if (which != __ITIMER_REAL) {
        ret.error = EINVAL;
        return ret;
    }

    struct __itimerval next;
    if (!usercopy_read(&next, new_value, sizeof(next))) {
        ret.error = EFAULT;
        return ret;
    }

    uint64_t value, interval;
    ret.error = time_tv_to_nano(&next.it_value, &value);
    if (!ret.error)
        ret.error = time_tv_to_nano(&next.it_interval, &interval);
    if (ret.error)
        return ret;

    struct process* proc = self();
    const struct __itimerval previous = current_itimer(proc);
    signal_itimer_set(proc, value, interval);

    if (old && !usercopy_write(old, &previous, sizeof(previous))) {
        ret.error = EFAULT;
        return ret;
    }
    return ret;
}

struct syscall_ret sys_getrlimit(int resource, __user struct __rlimit* rlim) {
    struct syscall_ret ret = {};

    if (resource < 0 || resource >= __RLIMIT_NLIMITS) {
        ret.error = EINVAL;
        return ret;
    }

    struct process* proc = self();
    struct __rlimit value = {__RLIM_INFINITY, __RLIM_INFINITY};
    mutex_lock(&proc->lock);
    struct process_rlimit* limit = rlimit_of(proc, resource);
    if (limit)
        value = (struct __rlimit){atomic_load(&limit->cur, ATOMIC_RELAXED), limit->max};
    mutex_unlock(&proc->lock);
    if (!usercopy_write(rlim, &value, sizeof(value)))
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_setrlimit(int resource, const __user struct __rlimit* rlim) {
    struct syscall_ret ret = {};

    struct __rlimit value;
    if (!usercopy_read(&value, rlim, sizeof(value))) {
        ret.error = EFAULT;
        return ret;
    }
    if (value.rlim_cur > value.rlim_max) {
        ret.error = EINVAL;
        return ret;
    }

    struct process* proc = self();
    if (!rlimit_of(proc, resource)) {
        ret.error = EINVAL;
        return ret;
    }
    if (resource == __RLIMIT_NOFILE && value.rlim_max > PROCESS_MAX_FDS && value.rlim_max != __RLIM_INFINITY) {
        ret.error = EPERM;
        return ret;
    }

    mutex_lock(&proc->lock);
    struct process_rlimit* limit = rlimit_of(proc, resource);
    if (value.rlim_max > limit->max && !ident_is_root(&proc->identity, false))
        ret.error = EPERM;
    else {
        limit->max = value.rlim_max;
        atomic_store(&limit->cur, value.rlim_cur, ATOMIC_RELAXED);
    }
    mutex_unlock(&proc->lock);
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

struct syscall_ret sys_reboot(uint32_t magic, uint32_t cmd) {
    struct syscall_ret ret = {};

    if (magic != __REBOOT_MAGIC) {
        ret.error = EINVAL;
        return ret;
    }
    if (!ident_is_root(&self()->identity, false)) {
        ret.error = EPERM;
        return ret;
    }

    ret.error = vfs_sync_all();
    errno_t block_status = blockdev_sync_all();
    if (ret.error || block_status)
        log_warn("reboot: Failed to sync file systems (%i, %i)\n", ret.error, block_status);

    switch (cmd) {
    case __RB_DISABLE_CAD:
        log_warn("reboot: RB_DISABLE_CAD is unimplemented\n");
        return ret;
    case __RB_ENABLE_CAD:
        log_warn("reboot: RB_ENABLE_CAD is unimplemented\n");
        return ret;
    case __RB_AUTOBOOT:
#if CONFIG_ACPI
        ret.error = acpi_reboot();
        return ret;
#else
        ret.error = ENOSYS;
        return ret;
#endif
    case __RB_POWER_OFF:
#if CONFIG_ACPI
        ret.error = acpi_power_off();
        return ret;
#else
        ret.error = ENOSYS;
        return ret;
#endif
    default:
        log_warn("reboot: Unknown command %#x\n", cmd);
        ret.error = EINVAL;
        return ret;
    }
}

struct syscall_ret sys_sleep(const __user struct __timespec* request, __user struct __timespec* remainder) {
    struct syscall_ret ret = {};

    uint64_t ns;
    ret.error = read_timespec_ns(request, &ns);
    if (ret.error || ns == 0)
        return ret;

    const uint64_t deadline = deadline_after(ns);
    struct poll_table table;
    poll_table_init(&table);
    do {
        ret.error = poll_table_wait(&table, &deadline);
        atomic_store(&table.triggered, false, ATOMIC_RELAXED);
    } while (ret.error == 0);
    poll_table_fini(&table);

    if (ret.error == ETIMEDOUT)
        ret.error = 0;
    if (remainder) {
        const uint64_t now = clock_get_elapsed_ns();
        const struct __timespec left = time_from_nano(ret.error && deadline > now ? deadline - now : 0);
        if (!usercopy_write(remainder, &left, sizeof(left)) && !ret.error)
            ret.error = EFAULT;
    }
    return ret;
}

struct syscall_ret sys_yield() {
    struct syscall_ret ret = {};

    sched_reschedule(&percpu_get()->sched);
    return ret;
}

struct syscall_ret sys_sysconf(int name) {
    struct syscall_ret ret = {};

    switch (name) {
    case __SC_ARG_MAX:
        ret.value = __ARG_MAX;
        return ret;
    case __SC_CHILD_MAX:
        ret.value = -1;
        return ret;
    case __SC_CLK_TCK:
        ret.value = 100;
        return ret;
    case __SC_NGROUPS_MAX:
        ret.value = __NGROUPS_MAX;
        return ret;
    case __SC_OPEN_MAX:
        ret.value = process_fd_limit(self());
        return ret;
    case __SC_PAGE_SIZE:
        ret.value = page_size();
        return ret;
    case __SC_NPROCESSORS_CONF:
    case __SC_NPROCESSORS_ONLN:
        ret.value = percpu_count();
        return ret;
    default:
        ret.error = EINVAL;
        return ret;
    }
}
