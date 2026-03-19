#include <kernel/alloc.h>
#include <kernel/assert.h>
#include <kernel/compiler.h>
#include <kernel/exec.h>
#include <kernel/identity.h>
#include <kernel/init.h>
#include <kernel/percpu.h>
#include <kernel/process.h>
#include <kernel/sched.h>
#include <kernel/spin.h>
#include <kernel/vm_space.h>
#include <uapi/errno.h>
#include <stdatomic.h>

struct process* kernel_process = nullptr;

static pid_t next_pid = 0;

errno_t process_new(struct process* parent, struct vm_space* space, struct process** out) {
    struct process* proc = mem_alloc(sizeof(*proc), 0);
    if (proc == nullptr)
        return ENOMEM;

    proc->refcount = 1;
    proc->id = atomic_fetch_add_explicit(&next_pid, 1, memory_order_relaxed);
    proc->parent = parent;
    proc->address_space = space;

    memset(&proc->children, 0, sizeof(proc->children));

    // If no parent is given, assume absolute control.
    if (parent == nullptr) {
        proc->identity = kernel_identity;
        proc->root_dir = vfs_root;
        proc->working_dir = vfs_root;
    } else {
        spin_lock(&parent->lock);
        proc->identity = parent->identity;
        proc->root_dir = parent->root_dir;
        proc->working_dir = parent->working_dir;
    }

    *out = proc;

    return 0;
}

errno_t process_exec(struct process* proc, struct file* executable, const char** argv, const char** envp) {
    struct exec_info info = {
        .executable = executable,
        .interpreter = nullptr, // We don't know the interpreter yet, a driver will fill this out.
        .space = vm_space_new(),
        .argv = argv,
        .envp = envp,
    };

    struct task* new_task;
    errno_t e = exec_file(&info, &new_task);
    if (e)
        return e;

    sched_yield(&percpu_get()->sched);
    __unreachable();
}

[[__init]]
void process_init() {}
