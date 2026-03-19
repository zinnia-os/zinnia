#pragma once

#include <kernel/process.h>
#include <kernel/vfs.h>
#include <kernel/vm_space.h>
#include <uapi/errno.h>

struct exec_info {
    struct file* executable;  // The excutable to load.
    struct file* interpreter; // An interpreter that's tasked with loading the given executable, optional.
    struct vm_space* space;   // An address space for the new process.
    const char** argv;
    const char** envp;
};

struct exec_format {
    // Determines if a file is a valid executable of this format.
    bool (*identify)(struct exec_format* format, struct file* file);

    // Loads the executable and returns a new initial thread.
    errno_t (*load)(struct exec_format* format, struct process* proc, struct exec_info* info, struct task** result);
};

errno_t exec_file(struct exec_info* info, struct task** result);

// Registers a new exec_format.
errno_t exec_register(const char* name, const struct exec_format* info);
