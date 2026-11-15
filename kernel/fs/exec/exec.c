// SPDX-License-Identifier: BSD-3-Clause

#include <fs/exec/exec.h>
#include <fs/vfs.h>
#include <lib/mutex.h>
#include <lib/print.h>
#include <lib/process.h>
#include <lib/tailq.h>

static mutex_t formats_lock;
static tailq_head(struct exec_format) formats = {nullptr, &formats.tqh_first};

struct exec_format* exec_identify(struct vmo* obj) {
    mutex_lock(&formats_lock);
    struct exec_format* format = tailq_first(&formats);
    while (format != nullptr && !format->identify(obj))
        format = tailq_next(format, link);
    mutex_unlock(&formats_lock);
    return format;
}

void exec_register(struct exec_format* format) {
    mutex_lock(&formats_lock);
    tailq_insert_tail(&formats, format, link);
    mutex_unlock(&formats_lock);

    log("exec: Registered new exec format \"%s\"\n", format->name);
}

errno_t exec_open(struct process* proc, const char* path, struct vmo** out) {
    mutex_lock(&proc->lock);
    struct vfs_path root = vfs_path_clone(proc->root);
    struct vfs_path cwd = vfs_path_clone(proc->cwd);
    mutex_unlock(&proc->lock);

    struct vfs_file* file;
    errno_t status = vfs_file_open(root, cwd, path, O_RDONLY | O_EXEC, 0, &proc->identity, &file);
    vfs_path_drop(root);
    vfs_path_drop(cwd);
    if (status)
        return status;

    status = exec_file_object(proc, file, out);
    vfs_file_unref(file);
    return status;
}

errno_t exec_file_object(struct process* proc, struct vfs_file* file, struct vmo** out) {
    errno_t status = vfs_node_check_exec(file->vnode, &proc->identity);
    if (!status)
        status = vfs_file_get_object(file, out);
    return status;
}
