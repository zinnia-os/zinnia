// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <lib/tailq.h>
#include <stddef.h>
#include <stdint.h>

struct process;
struct vfs_file;
struct vma;
struct vmo;

// Information passed to `exec_format.load`.
struct exec_info {
    // An address space for the new process.
    struct vma* space;
    // The excutable to load.
    struct vmo* file_obj;
    // Path passed to execve.
    const char* exec_path;
    // Arguments.
    const char** argv;
    // Environment variables.
    const char** envp;

    size_t depth;
};

// An executable format.
struct exec_format {
    const char* name;

    // Identifies whether a file is a valid executable of this format.
    bool (*identify)(struct vmo* obj);

    // Loads the executable and returns a new initial thread.
    // The implementation should not modify `proc` at all.
    errno_t (*load)(struct process* proc, struct exec_info* info, uintptr_t* out_ip, uintptr_t* out_sp);

    tailq_link(struct exec_format) link;
};

// Attempts to identify the format of this executable file.
struct exec_format* exec_identify(struct vmo* obj);

// Installs a new executable format.
void exec_register(struct exec_format* format);

errno_t exec_open(struct process* proc, const char* path, struct vmo** out);

errno_t exec_file_object(struct process* proc, struct vfs_file* file, struct vmo** out);
