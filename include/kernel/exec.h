#pragma once

#include <kernel/vfs.h>
#include <kernel/vm_object.h>
#include <kernel/vm_space.h>
#include <uapi/errno.h>

struct exec_info {
    struct vm_object* file_obj;
    struct vm_space* space;
    const char** argv;
    const char** envp;
};

struct exec_format {
    bool (*identify)(struct exec_format* self, struct vm_object* file);
    errno_t (*load)(struct exec_format* self, struct exec_info* info, struct task* result);
};
