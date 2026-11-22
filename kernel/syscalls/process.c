// SPDX-License-Identifier: BSD-3-Clause

#include <lib/atomic.h>
#include <lib/irq.h>
#include <lib/panic.h>
#include <lib/percpu.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <lib/signal.h>
#include <mem/alloc.h>
#include <mem/usercopy.h>
#include <syscalls/private.h>
#include <syscalls/syscall.h>
#include <string.h>
#include <zinnia/limits.h>
#include <zinnia/resource.h>
#include <zinnia/sched.h>
#include <zinnia/wait.h>

constexpr size_t NGROUPS_MAX = 65536;
constexpr uid_t UID_KEEP = (uid_t)-1;
constexpr gid_t GID_KEEP = (gid_t)-1;

static bool ident_has_uid(const struct ident* id, uid_t uid) {
    return uid == id->uid || uid == id->euid || uid == id->suid;
}

static bool ident_has_gid(const struct ident* id, gid_t gid) {
    return gid == id->gid || gid == id->egid || gid == id->sgid;
}

constexpr size_t EXEC_MAX_STRINGS = 4096;
constexpr size_t EXEC_MAX_BYTES = (256 * 1024);

static errno_t copy_string(const __user char* src, size_t max, size_t* budget, char** out) {
    size_t len;
    if (!usercopy_strlen(src, max, &len))
        return EFAULT;
    if (len >= max || len + 1 > *budget)
        return E2BIG;

    char* buf = mem_alloc(len + 1, ALLOC_NOZERO);
    if (!buf)
        return ENOMEM;
    if (!usercopy_read(buf, src, len) || strnlen(buf, len) != len) {
        mem_free(buf, len + 1);
        return EFAULT;
    }
    buf[len] = 0;
    *budget -= len + 1;
    *out = buf;
    return 0;
}

static void free_strings(char** strings) {
    if (!strings)
        return;
    size_t count = 0;
    for (; strings[count]; count++)
        mem_free(strings[count], strlen(strings[count]) + 1);
    mem_free(strings, (count + 1) * sizeof(char*));
}

static errno_t copy_strings(const __user uintptr_t* src, size_t* budget, char*** out) {
    char** strings = mem_alloc(sizeof(char*), 0);
    if (!strings)
        return ENOMEM;
    size_t count = 0;

    while (src) {
        uintptr_t ptr;
        if (!usercopy_read(&ptr, src + count, sizeof(ptr))) {
            free_strings(strings);
            return EFAULT;
        }
        if (!ptr)
            break;
        if (count + 1 >= EXEC_MAX_STRINGS) {
            free_strings(strings);
            return E2BIG;
        }

        char* str;
        errno_t status = copy_string((const __user char*)ptr, EXEC_MAX_BYTES, budget, &str);
        if (status) {
            free_strings(strings);
            return status;
        }

        char** grown = mem_alloc((count + 2) * sizeof(char*), 0);
        if (!grown) {
            mem_free(str, strlen(str) + 1);
            free_strings(strings);
            return ENOMEM;
        }
        memcpy(grown, strings, count * sizeof(char*));
        grown[count] = str;
        mem_free(strings, (count + 1) * sizeof(char*));
        strings = grown;
        count++;
    }

    *out = strings;
    return 0;
}

static errno_t copy_exec_args(
    const __user uintptr_t* argv_ptr,
    const __user uintptr_t* envp_ptr,
    size_t* budget,
    char*** argv,
    char*** envp
) {
    errno_t status = copy_strings(argv_ptr, budget, argv);
    if (!status)
        status = copy_strings(envp_ptr, budget, envp);
    return status;
}

static bool priority_matches(struct process* self, struct process* proc, int which, int who) {
    switch (which) {
    case __PRIO_PROCESS:
        return proc->pid == (who ? who : self->pid);
    case __PRIO_PGRP:
        return proc->pgid == (who ? who : self->pgid);
    case __PRIO_USER:
        return proc->identity.uid == (uid_t)(who ? who : (int)self->identity.uid);
    default:
        return false;
    }
}

static errno_t check_sched_target(pid_t pid) {
    if (pid < 0)
        return EINVAL;
    if (pid == 0)
        return 0;
    struct process* proc = process_lookup(pid);
    if (!proc)
        return ESRCH;
    process_unref(proc);
    return 0;
}

constexpr size_t THREAD_NAME_MAX = 16;

static errno_t with_thread(pid_t tid, char* name, size_t name_len, bool set) {
    struct process* proc = self();
    errno_t status = ESRCH;
    spin_lock(&proc->tasks_lock);
    for (struct task* task = tailq_first(&proc->tasks); task; task = tailq_next(task, process_link)) {
        if (task->id != (size_t)tid)
            continue;
        if (set) {
            memcpy(task->name, name, name_len + 1);
        } else {
            const size_t len = strnlen(task->name, sizeof(task->name));
            const size_t copy = MIN(len, name_len - 1);
            memcpy(name, task->name, copy);
            name[copy] = 0;
        }
        status = 0;
        break;
    }
    spin_unlock(&proc->tasks_lock);
    return status;
}

struct syscall_ret sys_gettid() {
    struct syscall_ret ret = {};

    ret.value = percpu_get()->sched.current->id;
    return ret;
}

struct syscall_ret sys_getpid() {
    struct syscall_ret ret = {};

    ret.value = self()->pid;
    return ret;
}

struct syscall_ret sys_getppid() {
    struct syscall_ret ret = {};

    ret.value = process_parent_pid(self());
    return ret;
}

struct syscall_ret sys_getuid() {
    struct syscall_ret ret = {};

    struct process* proc = self();
    mutex_lock(&proc->lock);
    ret.value = proc->identity.uid;
    mutex_unlock(&proc->lock);
    return ret;
}

struct syscall_ret sys_geteuid() {
    struct syscall_ret ret = {};

    struct process* proc = self();
    mutex_lock(&proc->lock);
    ret.value = proc->identity.euid;
    mutex_unlock(&proc->lock);
    return ret;
}

struct syscall_ret sys_getgid() {
    struct syscall_ret ret = {};

    struct process* proc = self();
    mutex_lock(&proc->lock);
    ret.value = proc->identity.gid;
    mutex_unlock(&proc->lock);
    return ret;
}

struct syscall_ret sys_getegid() {
    struct syscall_ret ret = {};

    struct process* proc = self();
    mutex_lock(&proc->lock);
    ret.value = proc->identity.egid;
    mutex_unlock(&proc->lock);
    return ret;
}

struct syscall_ret sys_getgroups(size_t size, __user gid_t* list) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    mutex_lock(&proc->lock);
    const size_t count = vec_length(&proc->identity.groups);
    gid_t* copy = count && size ? mem_alloc(count * sizeof(gid_t), 0) : nullptr;
    if (copy) {
        vec_foreach(&proc->identity.groups, i) {
            copy[i] = vec_elem_get(&proc->identity.groups, i);
        }
    }
    mutex_unlock(&proc->lock);

    if (size == 0) {
        ret.value = count;
        return ret;
    }
    if (size < count) {
        mem_free(copy, count * sizeof(gid_t));
        ret.error = EINVAL;
        return ret;
    }
    if (count && !copy) {
        ret.error = ENOMEM;
        return ret;
    }

    const bool ok = !count || usercopy_write(list, copy, count * sizeof(gid_t));
    mem_free(copy, count * sizeof(gid_t));
    ret.error = ok ? 0 : EFAULT;
    if (!ret.error)
        ret.value = count;
    return ret;
}

struct syscall_ret sys_setgroups(size_t size, const __user gid_t* list) {
    struct syscall_ret ret = {};

    if (size > NGROUPS_MAX) {
        ret.error = EINVAL;
        return ret;
    }

    struct process* proc = self();
    if (!ident_is_root(&proc->identity, false)) {
        ret.error = EPERM;
        return ret;
    }

    gid_t* groups = size ? mem_alloc(size * sizeof(gid_t), 0) : nullptr;
    if (size && !groups) {
        ret.error = ENOMEM;
        return ret;
    }
    if (size && !usercopy_read(groups, list, size * sizeof(gid_t))) {
        mem_free(groups, size * sizeof(gid_t));
        ret.error = EFAULT;
        return ret;
    }

    mutex_lock(&proc->lock);
    vec_clear(&proc->identity.groups);
    for (size_t i = 0; i < size && !ret.error; i++)
        ret.error = vec_push(&proc->identity.groups, groups[i]);
    mutex_unlock(&proc->lock);

    mem_free(groups, size * sizeof(gid_t));
    return ret;
}

struct syscall_ret sys_getresuid(__user uid_t* ruid, __user uid_t* euid, __user uid_t* suid) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    mutex_lock(&proc->lock);
    const uid_t values[3] = {proc->identity.uid, proc->identity.euid, proc->identity.suid};
    mutex_unlock(&proc->lock);

    if (!usercopy_write(ruid, &values[0], sizeof(values[0])) || !usercopy_write(euid, &values[1], sizeof(values[1])) ||
        !usercopy_write(suid, &values[2], sizeof(values[2])))
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_setuid(uid_t uid) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    struct ident* id = &proc->identity;
    mutex_lock(&proc->lock);
    if (ident_is_root(id, false)) {
        id->uid = uid;
        id->euid = uid;
        id->suid = uid;
    } else if (uid == id->uid || uid == id->suid) {
        id->euid = uid;
    } else {
        ret.error = EPERM;
    }
    mutex_unlock(&proc->lock);
    return ret;
}

struct syscall_ret sys_seteuid(uid_t euid) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    struct ident* id = &proc->identity;
    mutex_lock(&proc->lock);
    if (ident_is_root(id, false) || euid == id->uid || euid == id->suid)
        id->euid = euid;
    else
        ret.error = EPERM;
    mutex_unlock(&proc->lock);
    return ret;
}

struct syscall_ret sys_setresuid(uid_t ruid, uid_t euid, uid_t suid) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    struct ident* id = &proc->identity;
    mutex_lock(&proc->lock);
    if (!ident_is_root(id, false) &&
        ((ruid != UID_KEEP && !ident_has_uid(id, ruid)) || (euid != UID_KEEP && !ident_has_uid(id, euid)) ||
         (suid != UID_KEEP && !ident_has_uid(id, suid)))) {
        ret.error = EPERM;
    } else {
        if (ruid != UID_KEEP)
            id->uid = ruid;
        if (euid != UID_KEEP)
            id->euid = euid;
        if (suid != UID_KEEP)
            id->suid = suid;
    }
    mutex_unlock(&proc->lock);
    return ret;
}

struct syscall_ret sys_setreuid(uid_t ruid, uid_t euid) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    struct ident* id = &proc->identity;
    mutex_lock(&proc->lock);
    const bool root = ident_is_root(id, false);
    if (!root && ruid != UID_KEEP && ruid != id->uid && ruid != id->euid) {
        ret.error = EPERM;
    } else if (!root && euid != UID_KEEP && !ident_has_uid(id, euid)) {
        ret.error = EPERM;
    } else {
        // The saved ID reuses the effective ID whenever the real ID is set or the effective ID moves away from it.
        const uid_t old_uid = id->uid;
        if (ruid != UID_KEEP)
            id->uid = ruid;
        if (euid != UID_KEEP)
            id->euid = euid;
        if (ruid != UID_KEEP || (euid != UID_KEEP && euid != old_uid))
            id->suid = id->euid;
    }
    mutex_unlock(&proc->lock);
    return ret;
}

struct syscall_ret sys_getresgid(__user gid_t* rgid, __user gid_t* egid, __user gid_t* sgid) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    mutex_lock(&proc->lock);
    const gid_t values[3] = {proc->identity.gid, proc->identity.egid, proc->identity.sgid};
    mutex_unlock(&proc->lock);

    if (!usercopy_write(rgid, &values[0], sizeof(values[0])) || !usercopy_write(egid, &values[1], sizeof(values[1])) ||
        !usercopy_write(sgid, &values[2], sizeof(values[2])))
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_setgid(gid_t gid) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    struct ident* id = &proc->identity;
    mutex_lock(&proc->lock);
    if (ident_is_root(id, false)) {
        id->gid = gid;
        id->egid = gid;
        id->sgid = gid;
    } else if (gid == id->gid || gid == id->sgid) {
        id->egid = gid;
    } else {
        ret.error = EPERM;
    }
    mutex_unlock(&proc->lock);
    return ret;
}

struct syscall_ret sys_setegid(gid_t egid) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    struct ident* id = &proc->identity;
    mutex_lock(&proc->lock);
    if (ident_is_root(id, false) || egid == id->gid || egid == id->sgid)
        id->egid = egid;
    else
        ret.error = EPERM;
    mutex_unlock(&proc->lock);
    return ret;
}

struct syscall_ret sys_setresgid(gid_t rgid, gid_t egid, gid_t sgid) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    struct ident* id = &proc->identity;
    mutex_lock(&proc->lock);
    if (!ident_is_root(id, false) &&
        ((rgid != GID_KEEP && !ident_has_gid(id, rgid)) || (egid != GID_KEEP && !ident_has_gid(id, egid)) ||
         (sgid != GID_KEEP && !ident_has_gid(id, sgid)))) {
        ret.error = EPERM;
    } else {
        if (rgid != GID_KEEP)
            id->gid = rgid;
        if (egid != GID_KEEP)
            id->egid = egid;
        if (sgid != GID_KEEP)
            id->sgid = sgid;
    }
    mutex_unlock(&proc->lock);
    return ret;
}

struct syscall_ret sys_setregid(gid_t rgid, gid_t egid) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    struct ident* id = &proc->identity;
    mutex_lock(&proc->lock);
    const bool root = ident_is_root(id, false);
    if (!root && rgid != GID_KEEP && rgid != id->gid && rgid != id->egid) {
        ret.error = EPERM;
    } else if (!root && egid != GID_KEEP && !ident_has_gid(id, egid)) {
        ret.error = EPERM;
    } else {
        // The saved ID follows the effective one whenever the real ID is set or the effective one moves away from it.
        const gid_t old_gid = id->gid;
        if (rgid != GID_KEEP)
            id->gid = rgid;
        if (egid != GID_KEEP)
            id->egid = egid;
        if (rgid != GID_KEEP || (egid != GID_KEEP && egid != old_gid))
            id->sgid = id->egid;
    }
    mutex_unlock(&proc->lock);
    return ret;
}

struct syscall_ret sys_getpgid(pid_t pid) {
    struct syscall_ret ret = {};

    pid_t pgid = 0;
    ret.error = process_getpgid(self(), pid, &pgid);
    if (!ret.error)
        ret.value = pgid;
    return ret;
}

struct syscall_ret sys_setpgid(pid_t pid, pid_t pgid) {
    struct syscall_ret ret = {};

    ret.error = process_setpgid(self(), pid, pgid);
    return ret;
}

struct syscall_ret sys_getsid(pid_t pid) {
    struct syscall_ret ret = {};

    pid_t sid = 0;
    ret.error = process_getsid(self(), pid, &sid);
    if (!ret.error)
        ret.value = sid;
    return ret;
}

struct syscall_ret sys_setsid() {
    struct syscall_ret ret = {};

    pid_t sid = 0;
    ret.error = process_setsid(self(), &sid);
    if (!ret.error)
        ret.value = sid;
    return ret;
}

struct syscall_ret sys_getpriority(int which, int who) {
    struct syscall_ret ret = {};

    if (which < __PRIO_PROCESS || which > __PRIO_USER) {
        ret.error = EINVAL;
        return ret;
    }

    struct process* me = self();
    size_t count;
    struct process** procs = process_snapshot(&count);
    if (!procs) {
        ret.error = ENOMEM;
        return ret;
    }

    bool found = false;
    int nice = 19;
    for (size_t i = 0; i < count; i++) {
        if (procs[i]->state != PROCESS_ZOMBIE && priority_matches(me, procs[i], which, who)) {
            found = true;
            nice = MIN(nice, procs[i]->nice);
        }
        process_unref(procs[i]);
    }
    mem_free(procs, MAX(count, 1) * sizeof(*procs));
    ret.error = found ? 0 : ESRCH;
    if (!ret.error)
        ret.value = (uintptr_t)(intptr_t)nice;
    return ret;
}

struct syscall_ret sys_setpriority(int which, int who, int prio) {
    struct syscall_ret ret = {};

    if (which < __PRIO_PROCESS || which > __PRIO_USER) {
        ret.error = EINVAL;
        return ret;
    }

    struct process* me = self();
    const int nice = MAX(-20, MIN(19, prio));
    const uid_t euid = me->identity.euid;

    size_t count;
    struct process** procs = process_snapshot(&count);
    if (!procs) {
        ret.error = ENOMEM;
        return ret;
    }

    bool found = false;
    for (size_t i = 0; i < count; i++) {
        struct process* proc = procs[i];
        if (proc->state != PROCESS_ZOMBIE && priority_matches(me, proc, which, who)) {
            found = true;
            if (euid != 0 && euid != proc->identity.uid && euid != proc->identity.euid)
                ret.error = EPERM;
            else if (nice < proc->nice && euid != 0)
                ret.error = EACCES;
            else
                process_set_nice(proc, nice);
        }
        process_unref(proc);
    }
    mem_free(procs, MAX(count, 1) * sizeof(*procs));
    ret.error = found ? ret.error : ESRCH;
    return ret;
}

struct syscall_ret sys_sched_getparam(pid_t pid, __user struct __sched_param* param) {
    struct syscall_ret ret = {};

    ret.error = check_sched_target(pid);
    if (ret.error)
        return ret;
    const struct __sched_param value = {
        .sched_priority = 0,
        .sched_priority_min = 0,
        .sched_priority_max = 0,
    };
    if (!usercopy_write(param, &value, sizeof(value)))
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_sched_setparam(pid_t pid, const __user struct __sched_param* param) {
    struct syscall_ret ret = {};

    ret.error = check_sched_target(pid);
    if (ret.error)
        return ret;
    struct __sched_param value;
    if (!usercopy_read(&value, param, sizeof(value))) {
        ret.error = EFAULT;
        return ret;
    }
    if (!(value.sched_priority == 0))
        ret.error = EINVAL;
    return ret;
}

struct syscall_ret sys_exit(int status) {
    process_exit(status);
}

struct syscall_ret sys_fork(arch_context_t* ctx) {
    struct syscall_ret ret = {};

    pid_t pid = 0;
    ret.error = process_fork(ctx, &pid);
    if (!ret.error)
        ret.value = pid;
    return ret;
}

struct syscall_ret sys_execve(arch_context_t* ctx) {
    struct syscall_ret ret = {};

    size_t budget = EXEC_MAX_BYTES;
    char* path = nullptr;
    char** argv = nullptr;
    char** envp = nullptr;

    ret.error = copy_string((const __user char*)ctx->ARCH_CTX_A0, __PATH_MAX, &budget, &path);
    if (!ret.error) {
        ret.error = copy_exec_args(
            (const __user uintptr_t*)ctx->ARCH_CTX_A1,
            (const __user uintptr_t*)ctx->ARCH_CTX_A2,
            &budget,
            &argv,
            &envp
        );
    }
    if (!ret.error)
        ret.error = process_exec(path, (const char**)argv, (const char**)envp, ctx);

    if (path)
        mem_free(path, strlen(path) + 1);
    free_strings(argv);
    free_strings(envp);
    return ret;
}

struct syscall_ret sys_fexecve(arch_context_t* ctx) {
    struct syscall_ret ret = {};

    struct vfs_file* file = process_fd_get(self(), (int)ctx->ARCH_CTX_A0);
    if (!file) {
        ret.error = EBADF;
        return ret;
    }
    if (!(atomic_load(&file->flags, ATOMIC_RELAXED) & (O_RDONLY | O_EXEC))) {
        vfs_file_unref(file);
        ret.error = EBADF;
        return ret;
    }

    size_t budget = EXEC_MAX_BYTES;
    char** argv = nullptr;
    char** envp = nullptr;
    ret.error = copy_exec_args(
        (const __user uintptr_t*)ctx->ARCH_CTX_A1,
        (const __user uintptr_t*)ctx->ARCH_CTX_A2,
        &budget,
        &argv,
        &envp
    );
    if (!ret.error) {
        const char* exec_path = argv[0] ? argv[0] : "";
        ret.error = process_fexec(file, exec_path, (const char**)argv, (const char**)envp, ctx);
    }

    vfs_file_unref(file);
    free_strings(argv);
    free_strings(envp);
    return ret;
}

struct syscall_ret sys_waitpid(pid_t pid, __user int* stat_loc, int options, __user struct __rusage* rusage) {
    struct syscall_ret ret = {};

    if (options & ~(__WNOHANG | __WUNTRACED | __WCONTINUED)) {
        ret.error = EINVAL;
        return ret;
    }

    struct wait_result res;
    ret.error = process_wait(pid, options | __WEXITED, &res);
    if (ret.error)
        return ret;
    if (res.pid && stat_loc && !usercopy_write(stat_loc, &res.status, sizeof(res.status))) {
        ret.error = EFAULT;
        return ret;
    }
    if (res.pid && rusage) {
        const struct __rusage zero = {};
        if (!usercopy_write(rusage, &zero, sizeof(zero))) {
            ret.error = EFAULT;
            return ret;
        }
    }
    ret.value = res.pid;
    return ret;
}

struct syscall_ret sys_waitid(int idtype, pid_t id, __user struct __siginfo* info, int options) {
    struct syscall_ret ret = {};

    const int allowed = __WNOHANG | __WSTOPPED | __WEXITED | __WCONTINUED | __WNOWAIT;
    if ((options & ~allowed) || !(options & (__WEXITED | __WSTOPPED | __WCONTINUED))) {
        ret.error = EINVAL;
        return ret;
    }

    pid_t selector;
    switch (idtype) {
    case __P_ALL:
        selector = -1;
        break;
    case __P_PID:
        if (id <= 0) {
            ret.error = EINVAL;
            return ret;
        }
        selector = id;
        break;
    case __P_PGID:
        if (id < 0 || id == 1) {
            ret.error = EINVAL;
            return ret;
        }
        selector = id ? -id : 0;
        break;
    default:
        ret.error = EINVAL;
        return ret;
    }

    struct wait_result res;
    ret.error = process_wait(selector, options, &res);
    if (ret.error)
        return ret;

    if (info) {
        struct __siginfo out = {};
        if (res.pid) {
            out = (struct __siginfo){
                .si_signo = __SIGCHLD,
                .si_code = res.code,
                .si_pid = res.pid,
                .si_uid = res.uid,
                .si_status = res.info,
            };
        }
        if (!usercopy_write(info, &out, sizeof(out))) {
            ret.error = EFAULT;
            return ret;
        }
    }
    return ret;
}

struct syscall_ret sys_getrusage(int who, __user struct __rusage* usage) {
    struct syscall_ret ret = {};

    if (who != __RUSAGE_SELF && who != __RUSAGE_CHILDREN && who != __RUSAGE_THREAD) {
        ret.error = EINVAL;
        return ret;
    }
    const struct __rusage zero = {};
    if (!usercopy_write(usage, &zero, sizeof(zero)))
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_thread_create(uintptr_t entry, uintptr_t stack) {
    struct syscall_ret ret = {};

    size_t tid = 0;
    ret.error = process_thread_create(entry, stack, &tid);
    if (!ret.error)
        ret.value = tid;
    return ret;
}

struct syscall_ret sys_thread_exit() {
    process_thread_exit();
}

struct syscall_ret sys_thread_kill(pid_t pid, pid_t tid, signal_t sig) {
    struct syscall_ret ret = {};

    if ((sig != 0 && !sig_valid(sig)) || tid < 0) {
        ret.error = EINVAL;
        return ret;
    }

    struct process* sender = self();
    struct process* target = pid > 0 ? process_lookup(pid) : sender;
    if (!target) {
        ret.error = ESRCH;
        return ret;
    }
    if (target == sender)
        process_ref(target);

    ret.error = can_signal(sender, target, sig) ? 0 : EPERM;
    if (!ret.error) {
        const struct sig_info info = signal_info_user(sender, __SI_TKILL);
        if (!signal_send_thread(target, tid, sig, &info))
            ret.error = ESRCH;
    }
    process_unref(target);
    return ret;
}

struct syscall_ret sys_thread_setname(pid_t tid, const __user char* name) {
    struct syscall_ret ret = {};

    size_t len;
    if (!usercopy_strlen(name, THREAD_NAME_MAX, &len)) {
        ret.error = EFAULT;
        return ret;
    }
    if (len >= THREAD_NAME_MAX) {
        ret.error = ERANGE;
        return ret;
    }

    char buf[THREAD_NAME_MAX];
    if (!usercopy_read(buf, name, len)) {
        ret.error = EFAULT;
        return ret;
    }
    buf[len] = 0;
    ret.error = with_thread(tid, buf, len, true);
    return ret;
}

struct syscall_ret sys_umask(mode_t mask) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    mutex_lock(&proc->lock);
    ret.value = proc->umask;
    proc->umask = mask & 0777;
    mutex_unlock(&proc->lock);
    return ret;
}

struct syscall_ret sys_thread_getname(pid_t tid, __user char* name, size_t size) {
    struct syscall_ret ret = {};

    if (size == 0) {
        ret.error = ERANGE;
        return ret;
    }

    char buf[TASK_NAME_MAX];
    ret.error = with_thread(tid, buf, sizeof(buf), false);
    if (ret.error)
        return ret;
    const size_t len = strlen(buf);
    if (len + 1 > size) {
        ret.error = ERANGE;
        return ret;
    }
    if (!usercopy_write(name, buf, len + 1))
        ret.error = EFAULT;
    return ret;
}
