// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <fs/ident.h>
#include <fs/vfs.h>
#include <lib/atomic.h>
#include <lib/errno.h>
#include <lib/mutex.h>
#include <lib/refcount.h>
#include <lib/spin.h>
#include <lib/tailq.h>
#include <lib/types.h>
#include <stdint.h>
#include <zinnia/resource.h>

constexpr size_t PROCESS_DEFAULT_FDS = 1024;
constexpr size_t PROCESS_MAX_FDS = 1 << 20;

struct task;
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

struct process {
    struct refcount rc;
    pid_t pid;

    struct spinlock tasks_lock;
    tailq_head(struct task) tasks;
    size_t thread_count;

    mutex_t lock;
    struct vma* space;
    struct vfs_path root;
    struct vfs_path cwd;
    struct ident identity;
    mode_t umask;
    int8_t nice;

    struct process_rlimit limit_nofile;

    mutex_t files_lock;
    struct fd_table files;
};

struct process* process_kernel();
void process_ref(struct process* proc);
void process_unref(struct process* proc);

uint8_t process_priority(struct process* proc);
void process_attach_task(struct process* proc, struct task* task);
void process_detach_task(struct process* proc, struct task* task);

errno_t process_spawn_init(const char* path, const char** argv, const char** envp);

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
