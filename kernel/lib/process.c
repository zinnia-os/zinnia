// SPDX-License-Identifier: BSD-3-Clause

#include <fs/exec/exec.h>
#include <lib/atomic.h>
#include <lib/epoch.h>
#include <lib/panic.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/vma.h>
#include <mem/vmo.h>
#include <string.h>

constexpr size_t PROCESS_INITIAL_FDS = 64;

static struct process kernel_process;
static atomic_bool kernel_process_ready;
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
    spin_init(&proc->tasks_lock);
    tailq_init(&proc->tasks);
    mutex_init(&proc->lock);
    mutex_init(&proc->files_lock);
    vec_init(&proc->identity.groups);
    signal_process_init(proc);
}

struct process* process_kernel() {
    if (!atomic_load(&kernel_process_ready, ATOMIC_ACQUIRE)) {
        process_init_common(&kernel_process);
        kernel_process.pid = 0;
        kernel_process.space = &kernel_vma;
        atomic_store(&kernel_process_ready, true, ATOMIC_RELEASE);
    }
    return &kernel_process;
}

static void close_files(struct process* proc) {
    struct fd_table* t = &proc->files;
    mutex_lock(&proc->files_lock);
    for (size_t w = 0; w < fd_words(t->capacity); w++) {
        for (uint64_t bits = t->open[w]; bits; bits &= bits - 1)
            vfs_file_unref(atomic_load(fd_slot(t, w * 64 + __builtin_ctzll(bits)), ATOMIC_RELAXED));
    }
    fd_table_destroy(t);
    mutex_unlock(&proc->files_lock);
}

void process_ref(struct process* proc) {
    refcount_inc(&proc->rc);
}

void process_unref(struct process* proc) {
    if (proc == &kernel_process || !refcount_dec(&proc->rc))
        return;
    close_files(proc);
    vfs_path_drop(proc->root);
    vfs_path_drop(proc->cwd);
    if (proc->space)
        vma_unref(proc->space);
    vec_destroy(&proc->identity.groups);
    mem_free(proc, sizeof(*proc));
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
}

static void rlimit_init(struct process_rlimit* limit, rlim_t cur, rlim_t max) {
    atomic_init(&limit->cur, cur);
    limit->max = max;
}

static errno_t process_alloc(struct process** out) {
    struct process* proc = mem_alloc(sizeof(struct process), 0);
    if (!proc)
        return ENOMEM;
    process_init_common(proc);
    proc->pid = atomic_fetch_add(&next_pid, 1, ATOMIC_RELAXED);
    proc->root = vfs_get_root();
    proc->cwd = vfs_get_root();
    proc->umask = 022;
    rlimit_init(&proc->limit_nofile, PROCESS_DEFAULT_FDS, PROCESS_MAX_FDS);

    *out = proc;
    return 0;
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
    errno_t status = process_alloc(&proc);
    if (status)
        return status;

    struct task_start_info* start = mem_alloc(sizeof(struct task_start_info), 0);
    if (!start) {
        process_unref(proc);
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
        process_unref(proc);
        return status;
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
    process_unref(proc);
    if (status) {
        mem_free(start, sizeof(*start));
        return status;
    }

    sched_add_task(&percpu_get()->sched, task);
    return 0;
}

// TODO
void process_exit_signal(signal_t sig) {
    struct process* proc = percpu_get()->sched.current->process;
    panic("TODO");
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
