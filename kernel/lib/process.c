// SPDX-License-Identifier: BSD-3-Clause

#include <dev/tty/tty.h>
#include <fs/exec/exec.h>
#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/console.h>
#include <lib/epoch.h>
#include <lib/hashmap.h>
#include <lib/irq.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/process.h>
#include <lib/rwlock.h>
#include <lib/sched.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/vma.h>
#include <mem/vmo.h>
#include <syscalls/syscall.h>
#include <string.h>
#include <zinnia/wait.h>

static struct process kernel_process;
static atomic_bool kernel_process_ready;

static mutex_t tree_lock;
static mutex_t session_lock;
static struct rwlock pid_lock;
static hashmap(pid_t, struct process*) pid_table;
static atomic_int next_pid = 1;

struct fd_files {
    size_t capacity;
    struct epoch_head epoch;
    _Atomic(struct vfs_file*) slots[];
};

static size_t fd_words(size_t capacity) {
    return capacity / 64;
}

static void fd_files_free(struct epoch_head* head) {
    struct fd_files* files = containerof(head, struct fd_files, epoch);
    mem_free(files, sizeof(*files) + files->capacity * sizeof(files->slots[0]));
}

static _Atomic(struct vfs_file*)* fd_slot(struct fd_table* t, size_t fd) {
    return &atomic_load(&t->files, ATOMIC_RELAXED)->slots[fd];
}

static void fd_table_destroy(struct fd_table* t) {
    struct fd_files* files = atomic_load(&t->files, ATOMIC_RELAXED);
    atomic_store(&t->files, nullptr, ATOMIC_RELAXED);
    if (files)
        epoch_call(&files->epoch, fd_files_free);
    mem_free(t->open, fd_words(t->capacity) * sizeof(*t->open));
    mem_free(t->cloexec, fd_words(t->capacity) * sizeof(*t->cloexec));
    t->open = nullptr;
    t->cloexec = nullptr;
    t->capacity = 0;
    t->next_free = 0;
}

static errno_t fd_table_resize(struct fd_table* t, size_t capacity) {
    struct fd_files* files = mem_alloc(sizeof(*files) + capacity * sizeof(files->slots[0]), 0);
    uint64_t* open = mem_alloc(fd_words(capacity) * sizeof(*open), 0);
    uint64_t* cloexec = mem_alloc(fd_words(capacity) * sizeof(*cloexec), 0);
    if (!files || !open || !cloexec) {
        mem_free(files, sizeof(*files) + capacity * sizeof(files->slots[0]));
        mem_free(open, fd_words(capacity) * sizeof(*open));
        mem_free(cloexec, fd_words(capacity) * sizeof(*cloexec));
        return ENOMEM;
    }

    files->capacity = capacity;
    struct fd_files* old = atomic_load(&t->files, ATOMIC_RELAXED);
    if (old) {
        for (size_t i = 0; i < t->capacity; i++)
            atomic_store(&files->slots[i], atomic_load(&old->slots[i], ATOMIC_RELAXED), ATOMIC_RELAXED);
        memcpy(open, t->open, fd_words(t->capacity) * sizeof(*open));
        memcpy(cloexec, t->cloexec, fd_words(t->capacity) * sizeof(*cloexec));
    }
    mem_free(t->open, fd_words(t->capacity) * sizeof(*t->open));
    mem_free(t->cloexec, fd_words(t->capacity) * sizeof(*t->cloexec));

    t->open = open;
    t->cloexec = cloexec;
    t->capacity = capacity;
    epoch_publish(&t->files, files);
    if (old)
        epoch_call(&old->epoch, fd_files_free);
    return 0;
}

static errno_t fd_table_clone(struct fd_table* dst, struct fd_table* src) {
    if (!src->capacity)
        return 0;
    const errno_t status = fd_table_resize(dst, src->capacity);
    if (status)
        return status;

    for (size_t i = 0; i < src->capacity; i++)
        atomic_store(fd_slot(dst, i), atomic_load(fd_slot(src, i), ATOMIC_RELAXED), ATOMIC_RELAXED);
    memcpy(dst->open, src->open, fd_words(src->capacity) * sizeof(*dst->open));
    memcpy(dst->cloexec, src->cloexec, fd_words(src->capacity) * sizeof(*dst->cloexec));
    dst->next_free = src->next_free;

    for (size_t w = 0; w < fd_words(dst->capacity); w++) {
        for (uint64_t bits = dst->open[w]; bits; bits &= bits - 1)
            vfs_file_ref(atomic_load(fd_slot(dst, w * 64 + __builtin_ctzll(bits)), ATOMIC_RELAXED));
    }
    return 0;
}

static errno_t fd_table_grow(struct fd_table* t, size_t fd) {
    if (fd < t->capacity)
        return 0;
    size_t capacity = MAX(PROCESS_INITIAL_FDS, t->capacity);
    while (capacity <= fd)
        capacity *= 2;
    return fd_table_resize(t, MIN(capacity, PROCESS_MAX_FDS));
}

static size_t fd_find_free(struct fd_table* t, size_t start) {
    for (size_t w = start / 64; w < fd_words(t->capacity); w++) {
        uint64_t used = t->open[w];
        if (w == start / 64)
            used |= (1ull << (start % 64)) - 1;
        if (~used)
            return w * 64 + __builtin_ctzll(~used);
    }
    return MAX(start, t->capacity);
}

static void fd_set_cloexec(struct fd_table* t, size_t fd, bool cloexec) {
    const uint64_t bit = 1ull << (fd % 64);
    if (cloexec)
        t->cloexec[fd / 64] |= bit;
    else
        t->cloexec[fd / 64] &= ~bit;
}

static void fd_install(struct fd_table* t, size_t fd, struct vfs_file* file, bool cloexec) {
    t->open[fd / 64] |= 1ull << (fd % 64);
    fd_set_cloexec(t, fd, cloexec);
    epoch_publish(fd_slot(t, fd), file);
}

static void fd_clear(struct fd_table* t, size_t fd) {
    const uint64_t bit = 1ull << (fd % 64);
    atomic_store(fd_slot(t, fd), nullptr, ATOMIC_RELAXED);
    t->open[fd / 64] &= ~bit;
    t->cloexec[fd / 64] &= ~bit;
    if (fd < t->next_free)
        t->next_free = fd;
}

static struct vfs_file* fd_lookup(struct process* proc, int fd) {
    if (fd < 0 || (size_t)fd >= proc->files.capacity)
        return nullptr;
    return atomic_load(fd_slot(&proc->files, fd), ATOMIC_RELAXED);
}

static void process_init_common(struct process* proc) {
    refcount_init(&proc->rc);
    tailq_init(&proc->children);
    poll_event_init(&proc->child_event);
    spin_init(&proc->tasks_lock);
    tailq_init(&proc->tasks);
    poll_event_init(&proc->threads_event);
    mutex_init(&proc->lock);
    mutex_init(&proc->files_lock);
    vec_init(&proc->identity.groups);
}

struct process* process_kernel() {
    if (!atomic_load(&kernel_process_ready, ATOMIC_ACQUIRE)) {
        process_init_common(&kernel_process);
        kernel_process.pid = 0;
        kernel_process.space = &kernel_vma;
        signal_process_init(&kernel_process, nullptr);
        hashmap_init(&pid_table);
        atomic_store(&kernel_process_ready, true, ATOMIC_RELEASE);
    }
    return &kernel_process;
}

void process_ref(struct process* proc) {
    refcount_inc(&proc->rc);
}

void process_unref(struct process* proc) {
    if (proc == &kernel_process || !refcount_dec(&proc->rc))
        return;
    if (proc->space)
        vma_unref(proc->space);
    vec_destroy(&proc->identity.groups);
    fd_table_destroy(&proc->files);
    mem_free(proc, sizeof(*proc));
}

struct process* process_lookup(pid_t pid) {
    rwlock_read_lock(&pid_lock);
    struct process** found = hashmap_get(&pid_table, pid, hashmap_hash_default, hashmap_eq_default);
    struct process* proc = found ? *found : nullptr;
    if (proc)
        process_ref(proc);
    rwlock_read_unlock(&pid_lock);
    return proc;
}

pid_t process_parent_pid(struct process* proc) {
    mutex_lock(&tree_lock);
    pid_t pid = proc->parent ? proc->parent->pid : 0;
    mutex_unlock(&tree_lock);
    return pid;
}

uint8_t process_priority(struct process* proc) {
    if (proc == &kernel_process)
        return SCHED_PRIO_DEFAULT;

    // Normalize the priority from "nice" ranges to our priorities.
    return 16 + (19 - proc->nice) * 15 / 39;
}

void process_attach_task(struct process* proc, struct task* task) {
    spin_lock(&proc->tasks_lock);
    tailq_insert_tail(&proc->tasks, task, process_link);
    proc->thread_count++;
    spin_unlock(&proc->tasks_lock);
}

void process_detach_task(struct process* proc, struct task* task) {
    spin_lock(&proc->tasks_lock);
    tailq_remove(&proc->tasks, task, process_link);
    spin_unlock(&proc->tasks_lock);
    poll_event_notify(&proc->threads_event);
}

bool process_task_must_exit(struct task* task) {
    struct task* single = atomic_load(&task->process->single_task, ATOMIC_ACQUIRE);
    return single && single != task;
}

bool process_single_thread(struct process* proc) {
    struct task* self = percpu_get()->sched.current;
    struct task* expected = nullptr;
    if (!atomic_compare_exchange(&proc->single_task, &expected, self, ATOMIC_ACQ_REL, ATOMIC_ACQUIRE))
        return expected == self;

    struct poll_table table;
    poll_table_init(&table);
    table.interruptible = false;
    poll_wait(&table, &proc->threads_event);

    spin_lock(&proc->tasks_lock);
    for (struct task* t = tailq_first(&proc->tasks); t; t = tailq_next(t, process_link)) {
        if (t != self)
            sched_notify(t);
    }
    spin_unlock(&proc->tasks_lock);
    poll_event_notify(&proc->signals.cont_event);

    while (true) {
        // Stop waiting once we're the only task left in the process.
        spin_lock(&proc->tasks_lock);
        const bool only = tailq_first(&proc->tasks) == self && !tailq_next(self, process_link);
        spin_unlock(&proc->tasks_lock);
        if (only)
            break;

        poll_table_wait(&table, nullptr);
        atomic_store(&table.triggered, false, ATOMIC_RELAXED);
    }
    poll_table_fini(&table);

    spin_lock(&proc->tasks_lock);
    proc->thread_count = 1;
    spin_unlock(&proc->tasks_lock);
    return true;
}

static struct process* current_process() {
    return percpu_get()->sched.current->process;
}

static void rlimit_init(struct process_rlimit* limit, rlim_t cur, rlim_t max) {
    atomic_init(&limit->cur, cur);
    limit->max = max;
}

static void process_discard(struct process* proc) {
    // Drop the controlling terminal inherited at fork.
    mutex_lock(&session_lock);
    struct tty* tty = proc->ctty;
    proc->ctty = nullptr;
    mutex_unlock(&session_lock);
    if (tty)
        tty_unref(tty);
    vfs_path_drop(proc->root);
    vfs_path_drop(proc->cwd);
    process_unref(proc);
}

static errno_t process_alloc(struct process* parent, struct process** out) {
    struct process* proc = mem_alloc(sizeof(struct process), 0);
    if (!proc)
        return ENOMEM;
    process_init_common(proc);
    proc->pid = atomic_fetch_add(&next_pid, 1, ATOMIC_RELAXED);

    if (parent == &kernel_process) {
        proc->root = vfs_get_root();
        proc->cwd = vfs_get_root();
        proc->umask = 022;
        proc->pgid = proc->pid;
        rlimit_init(&proc->limit_nofile, PROCESS_DEFAULT_FDS, PROCESS_MAX_FDS);
        rlimit_init(&proc->limit_core, 0, __RLIM_INFINITY);
        signal_process_init(proc, nullptr);
        *out = proc;
        return 0;
    }

    mutex_lock(&parent->lock);
    proc->root = vfs_path_clone(parent->root);
    proc->cwd = vfs_path_clone(parent->cwd);
    proc->umask = parent->umask;
    proc->nice = parent->nice;
    rlimit_init(&proc->limit_nofile, atomic_load(&parent->limit_nofile.cur, ATOMIC_RELAXED), parent->limit_nofile.max);
    rlimit_init(&proc->limit_core, atomic_load(&parent->limit_core.cur, ATOMIC_RELAXED), parent->limit_core.max);
    const errno_t status = ident_copy(&proc->identity, &parent->identity);
    mutex_unlock(&parent->lock);
    if (status) {
        process_discard(proc);
        return status;
    }

    mutex_lock(&session_lock);
    proc->pgid = parent->pgid;
    proc->sid = parent->sid;
    proc->ctty = parent->ctty;
    if (proc->ctty)
        tty_ref(proc->ctty);
    mutex_unlock(&session_lock);

    signal_process_init(proc, parent);
    *out = proc;
    return 0;
}
static void process_publish(struct process* parent, struct process* proc) {
    rwlock_write_lock(&pid_lock);
    errno_t status = hashmap_insert(&pid_table, proc->pid, proc, hashmap_hash_default, hashmap_eq_default);
    assert(status == 0, "Out of memory while publishing process %d", proc->pid);
    rwlock_write_unlock(&pid_lock);

    mutex_lock(&tree_lock);
    proc->parent = parent;
    tailq_insert_tail(&parent->children, proc, sibling);
    mutex_unlock(&tree_lock);
}

static void close_files(struct process* proc, bool only_cloexec) {
    struct fd_table* t = &proc->files;
    mutex_lock(&proc->files_lock);
    for (size_t w = 0; w < fd_words(t->capacity); w++) {
        uint64_t bits = t->open[w];
        if (only_cloexec)
            bits &= t->cloexec[w];
        while (bits) {
            const size_t fd = w * 64 + __builtin_ctzll(bits);
            bits &= bits - 1;
            struct vfs_file* file = atomic_load(fd_slot(t, fd), ATOMIC_RELAXED);
            fd_clear(t, fd);
            vfs_file_unref(file);
        }
    }
    if (!only_cloexec)
        fd_table_destroy(t);
    mutex_unlock(&proc->files_lock);
}

static errno_t load_image(
    struct process* proc,
    struct vmo* object,
    const char* path,
    const char** argv,
    const char** envp,
    struct vma** out_space,
    uintptr_t* out_ip,
    uintptr_t* out_sp
) {
    struct vma* space;
    errno_t status = vma_new(&space);
    if (status)
        return status;

    struct exec_info info = {
        .space = space,
        .file_obj = object,
        .exec_path = path,
        .argv = argv,
        .envp = envp,
    };
    struct exec_format* format = exec_identify(object);
    status = format ? format->load(proc, &info, out_ip, out_sp) : ENOEXEC;
    if (status) {
        vma_unref(space);
        return status;
    }

    *out_space = space;
    return 0;
}

errno_t process_spawn_init(const char* path, const char** argv, const char** envp) {
    struct process* proc;
    errno_t status = process_alloc(process_kernel(), &proc);
    if (status)
        return status;

    struct task_start_info* start = mem_alloc(sizeof(struct task_start_info), 0);
    if (!start) {
        process_discard(proc);
        return ENOMEM;
    }

    struct vmo* object;
    status = exec_open(proc, path, &object);
    if (!status) {
        status = load_image(proc, object, path, argv, envp, &proc->space, &start->ip, &start->sp);
        vmo_unref(object);
    }
    if (status) {
        mem_free(start, sizeof(*start));
        process_discard(proc);
        return status;
    }

    proc->sid = proc->pid;
    struct tty* tty = console_get_tty();
    if (tty) {
        mutex_lock(&session_lock);
        tty->session = proc->sid;
        tty->pgrp = proc->pgid;
        proc->ctty = tty;
        mutex_unlock(&session_lock);
    }

    struct vfs_file* console;
    status = vfs_file_open(proc->root, proc->cwd, "/dev/console", O_RDWR, 0, ident_kernel(), &console);
    if (status) {
        log_warn("process: Unable to open /dev/console for init (%i)\n", status);
    } else {
        for (int fd = 0; fd <= 2; fd++)
            process_fd_replace(proc, fd, console, false);
        vfs_file_unref(console);
    }

    struct task* task;
    status = task_create(argv[0], proc, sched_to_user, (uintptr_t)start, &task);
    if (status) {
        mem_free(start, sizeof(*start));
        process_discard(proc);
        return status;
    }

    process_publish(process_kernel(), proc);
    sched_add_task(&percpu_get()->sched, task);
    return 0;
}

errno_t process_fork(arch_context_t* ctx, pid_t* out_pid) {
    struct task* self = percpu_get()->sched.current;
    struct process* parent = self->process;

    struct process* child;
    errno_t status = process_alloc(parent, &child);
    if (status)
        return status;

    arch_context_t* child_ctx = mem_alloc(sizeof(*child_ctx), 0);
    if (!child_ctx) {
        status = ENOMEM;
        goto err_child;
    }
    *child_ctx = *ctx;
    child_ctx->ARCH_CTX_RET0 = 0;
    child_ctx->ARCH_CTX_RET1 = 0;

    mutex_lock(&parent->lock);
    status = vma_fork(parent->space, &child->space);
    mutex_unlock(&parent->lock);
    if (status)
        goto err_ctx;

    mutex_lock(&parent->files_lock);
    status = fd_table_clone(&child->files, &parent->files);
    mutex_unlock(&parent->files_lock);
    if (status)
        goto err_files;

    struct task* task;
    status = task_create(self->name, child, sched_to_user_context, (uintptr_t)child_ctx, &task);
    if (status)
        goto err_files;
    task->user_stack = percpu_get()->user_stack;
    arch_task_fork(&task->context);
    signal_task_fork(task, self);

    process_publish(parent, child);
    *out_pid = child->pid;
    sched_add_task(&percpu_get()->sched, task);
    return 0;

err_files:
    close_files(child, false);
err_ctx:
    mem_free(child_ctx, sizeof(*child_ctx));
err_child:
    process_discard(child);
    return status;
}

static errno_t do_exec(
    struct vmo* object,
    const char* path,
    const char** argv,
    const char** envp,
    arch_context_t* ctx
) {
    struct task* self = percpu_get()->sched.current;
    struct process* proc = self->process;

    struct vma* space;
    uintptr_t ip, sp;
    errno_t status = load_image(proc, object, path, argv, envp, &space, &ip, &sp);
    vmo_unref(object);
    if (status)
        return status;

    if (!process_single_thread(proc)) {
        vma_unref(space);
        sched_exit();
    }

    close_files(proc, true);
    proc->did_exec = true;
    signal_exec(proc, self);

    vma_ref(space);
    mutex_lock(&proc->lock);
    struct vma* old = proc->space;
    proc->space = space;
    mutex_unlock(&proc->lock);

    irq_lock();
    self->space = space;
    pmap_set(&space->pmap);
    irq_unlock();

    vma_unref(old);
    vma_unref(old);

    strncpy(self->name, argv[0] ? argv[0] : path, sizeof(self->name) - 1);
    arch_context_init_user(ctx, ip, sp);
    atomic_store(&proc->single_task, nullptr, ATOMIC_RELEASE);
    return 0;
}

errno_t process_exec(const char* path, const char** argv, const char** envp, arch_context_t* ctx) {
    struct vmo* object;
    errno_t status = exec_open(current_process(), path, &object);
    if (status)
        return status;
    return do_exec(object, path, argv, envp, ctx);
}

errno_t process_fexec(
    struct vfs_file* file,
    const char* exec_path,
    const char** argv,
    const char** envp,
    arch_context_t* ctx
) {
    struct vmo* object;
    errno_t status = exec_file_object(current_process(), file, &object);
    if (status)
        return status;
    return do_exec(object, exec_path, argv, envp, ctx);
}

static void thread_start(uintptr_t info) {
    if (process_task_must_exit(percpu_get()->sched.current)) {
        mem_free((void*)info, sizeof(struct task_start_info));
        sched_exit();
    }
    sched_to_user(info);
}

errno_t process_thread_create(uintptr_t ip, uintptr_t sp, size_t* out_tid) {
    struct task* self = percpu_get()->sched.current;
    struct task_start_info* start = mem_alloc(sizeof(*start), 0);
    if (!start)
        return ENOMEM;
    start->ip = ip;
    start->sp = sp;

    struct task* task;
    errno_t status = task_create(self->name, self->process, thread_start, (uintptr_t)start, &task);
    if (status) {
        mem_free(start, sizeof(*start));
        return status;
    }
    atomic_store(&task->signals.mask, atomic_load(&self->signals.mask, ATOMIC_RELAXED), ATOMIC_RELAXED);

    *out_tid = task->id;
    sched_add_task(&percpu_get()->sched, task);
    return 0;
}

void process_thread_exit() {
    struct process* proc = current_process();
    spin_lock(&proc->tasks_lock);
    const bool last = --proc->thread_count == 0;
    spin_unlock(&proc->tasks_lock);

    if (last)
        process_exit(0);
    sched_exit();
}

[[noreturn]]
static void do_exit(int wait_status, int code, int info) {
    struct process* proc = current_process();
    assert(proc != &kernel_process, "The kernel process can't exit");
    assert(proc->pid != 1, "Attempted to kill init with status %#x", wait_status);

    if (!process_single_thread(proc))
        sched_exit();

    signal_process_fini(proc);
    close_files(proc, false);

    // Drop the controlling terminal, and detach it from the session if we lead it.
    mutex_lock(&session_lock);
    struct tty* tty = proc->ctty;
    proc->ctty = nullptr;
    if (tty && proc->sid == proc->pid)
        tty_detach_session_locked(tty);
    mutex_unlock(&session_lock);
    if (tty)
        tty_unref(tty);
    mutex_lock(&proc->lock);
    vfs_path_drop(proc->root);
    vfs_path_drop(proc->cwd);
    proc->root = (struct vfs_path){};
    proc->cwd = (struct vfs_path){};
    struct vma* space = proc->space;
    proc->space = nullptr;
    mutex_unlock(&proc->lock);
    vma_unref(space);

    struct process* init = process_lookup(1);

    mutex_lock(&tree_lock);
    struct process* child;
    bool zombies = false;
    while ((child = tailq_first(&proc->children))) {
        tailq_remove(&proc->children, child, sibling);
        child->parent = init;
        tailq_insert_tail(&init->children, child, sibling);
        zombies |= child->state == PROCESS_ZOMBIE;
    }

    proc->state = PROCESS_ZOMBIE;
    proc->exit_status = wait_status;
    proc->exit_code = code;
    proc->exit_info = info;
    mutex_unlock(&tree_lock);

    if (zombies)
        poll_event_notify(&init->child_event);
    process_unref(init);
    process_notify_parent(proc, code, info);
    sched_exit();
}

void process_exit(int status) {
    do_exit((status & 0xFF) << 8, __CLD_EXITED, status & 0xFF);
}

void process_exit_signal(signal_t sig) {
    do_exit(sig & 0x7F, __CLD_KILLED, sig);
}

void process_notify_parent(struct process* proc, int code, int status) {
    mutex_lock(&tree_lock);
    struct process* parent = proc->parent;
    if (parent)
        process_ref(parent);
    mutex_unlock(&tree_lock);

    if (!parent)
        return;
    if (parent == &kernel_process) {
        process_unref(parent);
        return;
    }

    poll_event_notify(&parent->child_event);

    spin_lock(&parent->signals.lock);
    const bool quiet = (code == __CLD_STOPPED || code == __CLD_CONTINUED) &&
        (parent->signals.actions[SIGCHLD - 1].sa_flags & __SA_NOCLDSTOP);
    spin_unlock(&parent->signals.lock);

    if (!quiet) {
        const struct sig_info info = {
            .code = code,
            .pid = proc->pid,
            .uid = proc->identity.uid,
            .status = status,
        };
        signal_send(parent, SIGCHLD, &info);
    }
    process_unref(parent);
}

// Report a stop or continue of `child` that its parent hasn't waited for yet.
static bool take_stop_or_continue(struct process* child, int options, struct wait_result* out) {
    struct process_signals* s = &child->signals;
    const bool consume = !(options & __WNOWAIT);
    bool found = false;

    spin_lock(&s->lock);
    if ((options & __WSTOPPED) && atomic_load(&s->stopped, ATOMIC_RELAXED) && s->stop_unwaited) {
        *out = (struct wait_result){
            .pid = child->pid,
            .uid = child->identity.uid,
            .status = (s->stop_sig << 8) | 0x7F,
            .code = __CLD_STOPPED,
            .info = s->stop_sig,
        };
        s->stop_unwaited = !consume;
        found = true;
    } else if ((options & __WCONTINUED) && s->cont_unwaited) {
        *out = (struct wait_result){
            .pid = child->pid,
            .uid = child->identity.uid,
            .status = 0xFFFF,
            .code = __CLD_CONTINUED,
            .info = SIGCONT,
        };
        s->cont_unwaited = !consume;
        found = true;
    }
    spin_unlock(&s->lock);
    return found;
}

errno_t process_wait(pid_t selector, int options, struct wait_result* out) {
    struct process* self = current_process();
    *out = (struct wait_result){};

    struct poll_table table;
    poll_table_init(&table);
    poll_wait(&table, &self->child_event);

    errno_t status = 0;
    mutex_lock(&tree_lock);
    while (true) {
        bool found = false;
        struct process* reaped = nullptr;
        bool reported = false;
        for (struct process* child = tailq_first(&self->children); child; child = tailq_next(child, sibling)) {
            const pid_t pgid = selector == 0 ? self->pgid : -selector;
            if (selector > 0 ? child->pid != selector : selector != -1 && child->pgid != pgid)
                continue;
            found = true;

            if (child->state == PROCESS_ZOMBIE) {
                if (!(options & __WEXITED))
                    continue;
                *out = (struct wait_result){
                    .pid = child->pid,
                    .uid = child->identity.uid,
                    .status = child->exit_status,
                    .code = child->exit_code,
                    .info = child->exit_info,
                };
                if (!(options & __WNOWAIT)) {
                    tailq_remove(&self->children, child, sibling);
                    reaped = child;
                }
                reported = true;
                break;
            }

            if (take_stop_or_continue(child, options, out)) {
                reported = true;
                break;
            }
        }

        if (reported) {
            mutex_unlock(&tree_lock);
            if (reaped) {
                rwlock_write_lock(&pid_lock);
                hashmap_remove(&pid_table, reaped->pid, hashmap_hash_default, hashmap_eq_default);
                rwlock_write_unlock(&pid_lock);
                process_unref(reaped);
            }
            break;
        }

        if (!found) {
            status = ECHILD;
            mutex_unlock(&tree_lock);
            break;
        }

        if (options & __WNOHANG) {
            mutex_unlock(&tree_lock);
            break;
        }

        if (poll_table_sleep(&table, &tree_lock, nullptr) == EINTR) {
            status = ERESTART;
            mutex_unlock(&tree_lock);
            break;
        }
    }

    poll_table_fini(&table);
    return status;
}

struct process** process_snapshot(size_t* out_count) {
    rwlock_read_lock(&pid_lock);
    const size_t capacity = hashmap_count(&pid_table);
    struct process** procs = mem_alloc(MAX(capacity, 1) * sizeof(*procs), 0);
    size_t count = 0;
    if (procs) {
        hashmap_foreach(&pid_table, i) {
            struct process* proc = pid_table.values[i];
            process_ref(proc);
            procs[count++] = proc;
        }
    }
    rwlock_read_unlock(&pid_lock);

    *out_count = count;
    return procs;
}

bool process_pgrp_orphaned(pid_t pgid, pid_t sid) {
    size_t count;
    struct process** procs = process_snapshot(&count);
    if (!procs)
        return false;

    bool orphaned = true;
    mutex_lock(&tree_lock);
    for (size_t i = 0; i < count && orphaned; i++) {
        struct process* proc = procs[i];
        struct process* parent = proc->parent;
        if (proc->pgid == pgid && proc->state != PROCESS_ZOMBIE && parent && parent != &kernel_process &&
            parent->pgid != pgid && parent->sid == sid)
            orphaned = false;
    }
    mutex_unlock(&tree_lock);

    for (size_t i = 0; i < count; i++)
        process_unref(procs[i]);
    mem_free(procs, MAX(count, 1) * sizeof(*procs));
    return orphaned;
}

void process_set_nice(struct process* proc, int nice) {
    proc->nice = MAX(-20, MIN(19, nice));
    spin_lock(&proc->tasks_lock);
    for (struct task* task = tailq_first(&proc->tasks); task; task = tailq_next(task, process_link))
        sched_set_priority(task, process_priority(proc));
    spin_unlock(&proc->tasks_lock);
}

void process_session_lock() {
    mutex_lock(&session_lock);
}

void process_session_unlock() {
    mutex_unlock(&session_lock);
}

static bool any_process(bool (*match)(struct process* proc, pid_t a, pid_t b), pid_t a, pid_t b) {
    bool found = false;
    rwlock_read_lock(&pid_lock);
    hashmap_foreach(&pid_table, i) {
        if (match(pid_table.values[i], a, b)) {
            found = true;
            break;
        }
    }
    rwlock_read_unlock(&pid_lock);
    return found;
}

static struct process* lookup_or_self(struct process* proc, pid_t pid) {
    if (pid == 0 || pid == proc->pid) {
        process_ref(proc);
        return proc;
    }
    return process_lookup(pid);
}

errno_t process_getpgid(struct process* proc, pid_t pid, pid_t* out_pgid) {
    struct process* target = lookup_or_self(proc, pid);
    if (!target)
        return ESRCH;

    mutex_lock(&session_lock);
    *out_pgid = target->pgid;
    mutex_unlock(&session_lock);
    process_unref(target);
    return 0;
}

errno_t process_getsid(struct process* proc, pid_t pid, pid_t* out_sid) {
    struct process* target = lookup_or_self(proc, pid);
    if (!target)
        return ESRCH;

    mutex_lock(&session_lock);
    *out_sid = target->sid;
    mutex_unlock(&session_lock);
    process_unref(target);
    return 0;
}

static bool in_pgrp_and_session(struct process* proc, pid_t pgid, pid_t sid) {
    return proc->pgid == pgid && proc->sid == sid;
}

bool process_pgrp_in_session(pid_t pgid, pid_t sid) {
    return any_process(in_pgrp_and_session, pgid, sid);
}

static bool in_pgrp_other(struct process* proc, pid_t pgid, pid_t self) {
    return proc->pgid == pgid && proc->pid != self;
}

void process_clear_ctty(pid_t sid, struct tty* tty) {
    rwlock_read_lock(&pid_lock);
    hashmap_foreach(&pid_table, i) {
        struct process* proc = pid_table.values[i];
        if (proc->sid == sid && proc->ctty == tty) {
            proc->ctty = nullptr;
            tty_unref(tty);
        }
    }
    rwlock_read_unlock(&pid_lock);
}

errno_t process_setsid(struct process* proc, pid_t* out_sid) {
    mutex_lock(&session_lock);
    const bool leader = proc->sid == proc->pid || (proc->pgid == proc->pid && proc->sid != 0);
    if (leader || any_process(in_pgrp_other, proc->pid, proc->pid)) {
        mutex_unlock(&session_lock);
        return EPERM;
    }

    struct tty* tty = proc->ctty;
    proc->pgid = proc->pid;
    proc->sid = proc->pid;
    proc->ctty = nullptr;
    mutex_unlock(&session_lock);

    if (tty)
        tty_unref(tty);
    *out_sid = proc->pid;
    return 0;
}

errno_t process_setpgid(struct process* proc, pid_t pid, pid_t pgid) {
    if (pgid < 0)
        return EINVAL;

    struct process* target = lookup_or_self(proc, pid);
    if (!target)
        return ESRCH;
    mutex_lock(&tree_lock);
    const bool child = target->parent == proc;
    mutex_unlock(&tree_lock);
    if (target != proc && !child) {
        process_unref(target);
        return ESRCH;
    }

    errno_t status = 0;
    mutex_lock(&session_lock);
    const pid_t new_pgid = pgid ? pgid : target->pid;
    if (target != proc && target->sid != proc->sid)
        status = EPERM;
    else if (target != proc && target->did_exec)
        status = EACCES;
    else if (target->sid == target->pid)
        status = EPERM;
    else if (new_pgid != target->pid && !any_process(in_pgrp_and_session, new_pgid, target->sid))
        status = EPERM;
    else
        target->pgid = new_pgid;
    mutex_unlock(&session_lock);

    process_unref(target);
    return status;
}

errno_t process_fd_alloc(struct process* proc, struct vfs_file* file, bool cloexec, int* out_fd) {
    return process_fd_alloc_min(proc, file, 0, cloexec, out_fd);
}

size_t process_fd_limit(struct process* proc) {
    return MIN((size_t)PROCESS_MAX_FDS, (size_t)atomic_load(&proc->limit_nofile.cur, ATOMIC_RELAXED));
}

errno_t process_fd_alloc_min(struct process* proc, struct vfs_file* file, int min_fd, bool cloexec, int* out_fd) {
    if (min_fd < 0 || (size_t)min_fd >= process_fd_limit(proc))
        return EINVAL;

    struct fd_table* t = &proc->files;
    mutex_lock(&proc->files_lock);
    const size_t fd = fd_find_free(t, MAX((size_t)min_fd, t->next_free));
    if (fd >= process_fd_limit(proc)) {
        mutex_unlock(&proc->files_lock);
        return EMFILE;
    }

    errno_t status = fd_table_grow(t, fd);
    if (status) {
        mutex_unlock(&proc->files_lock);
        return status;
    }
    fd_install(t, fd, file, cloexec);
    if ((size_t)min_fd <= t->next_free)
        t->next_free = fd + 1;
    vfs_file_ref(file);
    mutex_unlock(&proc->files_lock);

    *out_fd = fd;
    return 0;
}

errno_t process_fd_replace(struct process* proc, int fd, struct vfs_file* file, bool cloexec) {
    if (fd < 0 || (size_t)fd >= process_fd_limit(proc))
        return EBADF;

    struct fd_table* t = &proc->files;
    mutex_lock(&proc->files_lock);
    errno_t status = fd_table_grow(t, fd);
    if (status) {
        mutex_unlock(&proc->files_lock);
        return status;
    }
    struct vfs_file* displaced = fd_lookup(proc, fd);
    fd_install(t, fd, file, cloexec);
    vfs_file_ref(file);
    mutex_unlock(&proc->files_lock);

    if (displaced)
        vfs_file_unref(displaced);
    return 0;
}

errno_t process_fd_get_cloexec(struct process* proc, int fd, bool* out_cloexec) {
    errno_t status = EBADF;
    mutex_lock(&proc->files_lock);
    if (fd_lookup(proc, fd)) {
        *out_cloexec = (proc->files.cloexec[fd / 64] >> (fd % 64)) & 1;
        status = 0;
    }
    mutex_unlock(&proc->files_lock);
    return status;
}

errno_t process_fd_set_cloexec(struct process* proc, int fd, bool cloexec) {
    errno_t status = EBADF;
    mutex_lock(&proc->files_lock);
    struct vfs_file* file = fd_lookup(proc, fd);
    if (file) {
        fd_set_cloexec(&proc->files, fd, cloexec);
        status = 0;
    }
    mutex_unlock(&proc->files_lock);
    return status;
}

struct vfs_file* process_fd_get(struct process* proc, int fd) {
    if (fd < 0)
        return nullptr;

    struct vfs_file* file = nullptr;
    epoch_enter();
    struct fd_files* files = epoch_deref(&proc->files.files);
    if (files && (size_t)fd < files->capacity) {
        file = epoch_deref(&files->slots[fd]);
        if (file && !refcount_inc_not_zero(&file->rc))
            file = nullptr;
    }
    epoch_exit();
    return file;
}

errno_t process_fd_close(struct process* proc, int fd) {
    mutex_lock(&proc->files_lock);
    struct vfs_file* file = fd_lookup(proc, fd);
    if (file)
        fd_clear(&proc->files, fd);
    mutex_unlock(&proc->files_lock);

    if (!file)
        return EBADF;
    vfs_file_unref(file);
    return 0;
}
