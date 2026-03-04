#pragma once

#include <zinnia/status.h>
#include <kernel/namespace.h>
#include <kernel/sched.h>
#include <kernel/vmspace.h>
#include <kernel/vmo.h>

struct exec_info {
    struct vmspace* space;
    struct vmo* file_obj;
    struct namespace* ns;
    const char** argv;
    const char** envp;
};

zn_status_t elf_load(struct exec_info* info, struct task** out);
