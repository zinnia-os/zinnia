#pragma once

#include <kernel/vfs.h>
#include <kernel/vmobject.h>
#include <kernel/vmspace.h>
#include <uapi/errno.h>

struct exec_info {
    struct vmobject* file_obj;
    struct vmspace* space;
    const char** argv;
    const char** envp;
};

struct exec_format {
    bool (*identify)(struct exec_format* self, struct vmobject* file);
    errno_t (*load)(struct exec_format* self, struct exec_info* info, struct task* result);
};
