#pragma once

#include <kernel/identity.h>
#include <kernel/sched.h>
#include <kernel/spin.h>
#include <kernel/vec.h>
#include <kernel/vfs.h>
#include <uapi/types.h>

struct process {
    size_t refcount;
    pid_t id;
    struct process* parent; // If null, this process is the root process.
    VEC(struct process*) children;
    VEC(struct task*) threads;
    struct vm_space* address_space;
    struct spinlock lock;
    struct path root_dir;
    struct path working_dir;
    struct identity identity;
};

extern struct process* kernel_process;

// Creates a new process.
errno_t process_new(struct process* parent, struct vm_space* space, struct process** out);

errno_t process_fork(struct process* proc, struct arch_context* context);

[[noreturn]]
void process_exit(struct process* proc, uint8_t code);

errno_t process_exec(struct process* proc, struct file* executable, const char** argv, const char** envp);

void process_init();
