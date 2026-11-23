// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <fs/ident.h>
#include <fs/poll.h>
#include <fs/vfs.h>
#include <lib/atomic.h>
#include <lib/condvar.h>
#include <lib/errno.h>
#include <lib/mutex.h>
#include <lib/refcount.h>
#include <lib/signal.h>
#include <lib/spin.h>
#include <lib/tailq.h>
#include <lib/types.h>
#include <bits/sched.h>
#include <stdint.h>
#include <zinnia/resource.h>

constexpr size_t PROCESS_INITIAL_FDS = 64;
constexpr size_t PROCESS_DEFAULT_FDS = 1024;
constexpr size_t PROCESS_MAX_FDS = 1 << 20;

struct task;
struct tty;
struct vma;

struct process_rlimit {
    _Atomic(rlim_t) cur;
    rlim_t max;
};

struct fd_files;

struct fd_table {
    _Atomic(struct fd_files*) files;
    uint64_t* open;
    uint64_t* cloexec;
    size_t capacity;
    size_t next_free;
};

enum process_state {
    PROCESS_RUNNING,
    PROCESS_ZOMBIE,
};

struct process {
    struct refcount rc;
    pid_t pid;

    struct process* parent;
    tailq_head(struct process) children;
    tailq_link(struct process) sibling;
    enum process_state state;
    int exit_status;
    int exit_code;
    int exit_info;
    struct poll_event child_event;

    struct spinlock tasks_lock;
    tailq_head(struct task) tasks;
    size_t thread_count;
    _Atomic(struct task*) single_task;
    struct poll_event threads_event;

    mutex_t lock;
    struct vma* space;
    struct vfs_path root;
    struct vfs_path cwd;
    struct ident identity;
    mode_t umask;
    int8_t nice;
    bool did_exec;

    pid_t pgid;
    pid_t sid;
    struct tty* ctty;

    struct process_signals signals;
    struct process_rlimit limit_nofile;
    struct process_rlimit limit_core;

    mutex_t files_lock;
    struct fd_table files;
};

struct process* process_kernel();
void process_ref(struct process* proc);
void process_unref(struct process* proc);
struct process* process_lookup(pid_t pid);
pid_t process_parent_pid(struct process* proc);

uint8_t process_priority(struct process* proc);
void process_attach_task(struct process* proc, struct task* task);
void process_detach_task(struct process* proc, struct task* task);
bool process_single_thread(struct process* proc);
bool process_task_must_exit(struct task* task);

errno_t process_spawn_init(const char* path, const char** argv, const char** envp);
errno_t process_fork(arch_context_t* ctx, pid_t* out_pid);
errno_t process_exec(const char* path, const char** argv, const char** envp, arch_context_t* ctx);
errno_t process_fexec(
    struct vfs_file* file,
    const char* exec_path,
    const char** argv,
    const char** envp,
    arch_context_t* ctx
);
errno_t process_thread_create(uintptr_t ip, uintptr_t sp, size_t* out_tid);
[[noreturn]]
void process_thread_exit();
[[noreturn]]
void process_exit(int status);
[[noreturn]]
void process_exit_signal(signal_t sig);

struct wait_result {
    pid_t pid;
    uid_t uid;
    int status;
    int code;
    int info;
};

errno_t process_wait(pid_t selector, int options, struct wait_result* out);
void process_notify_parent(struct process* proc, int code, int status);
struct process** process_snapshot(size_t* out_count);
bool process_pgrp_orphaned(pid_t pgid, pid_t sid);
void process_set_nice(struct process* proc, int nice);

void process_session_lock();
void process_session_unlock();
bool process_pgrp_in_session(pid_t pgid, pid_t sid);
void process_clear_ctty(pid_t sid, struct tty* tty);

errno_t process_setsid(struct process* proc, pid_t* out_sid);
errno_t process_setpgid(struct process* proc, pid_t pid, pid_t pgid);
errno_t process_getpgid(struct process* proc, pid_t pid, pid_t* out_pgid);
errno_t process_getsid(struct process* proc, pid_t pid, pid_t* out_sid);

size_t process_fd_limit(struct process* proc);

// Allocate a file drescriptor for `file` on `proc`.
errno_t process_fd_alloc(struct process* proc, struct vfs_file* file, bool cloexec, int* out_fd);

// Allocate a file descriptor for `file` on `proc`, starting at at least `min_fd`.
errno_t process_fd_alloc_min(struct process* proc, struct vfs_file* file, int min_fd, bool cloexec, int* out_fd);

// Replaces the current file indentified by `fd` in `proc` with a new `file`.
errno_t process_fd_replace(struct process* proc, int fd, struct vfs_file* file, bool cloexec);
errno_t process_fd_get_cloexec(struct process* proc, int fd, bool* out_cloexec);
errno_t process_fd_set_cloexec(struct process* proc, int fd, bool cloexec);
struct vfs_file* process_fd_get(struct process* proc, int fd);
errno_t process_fd_close(struct process* proc, int fd);
