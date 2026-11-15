// SPDX-License-Identifier: BSD-3-Clause

#include <lib/module.h>
#include <lib/print.h>
#include <mem/alloc.h>
#include <mem/usercopy.h>
#include <syscalls/private.h>

constexpr size_t MODULE_MAX_SIZE = (64 * 1024 * 1024);
constexpr size_t MODULE_CMDLINE_MAX = 4096;
constexpr size_t MODULE_NAME_MAX = 256;

static errno_t read_module_file(struct process* proc, const char* path, void** out_data, size_t* out_len) {
    mutex_lock(&proc->lock);
    struct vfs_path root = vfs_path_clone(proc->root);
    struct vfs_path cwd = vfs_path_clone(proc->cwd);
    mutex_unlock(&proc->lock);

    struct vfs_file* file;
    errno_t status = vfs_file_open(root, cwd, path, O_RDONLY, 0, &proc->identity, &file);
    vfs_path_drop(root);
    vfs_path_drop(cwd);
    if (status)
        return status;

    const size_t len = file->vnode->size;
    void* data = len && len <= MODULE_MAX_SIZE ? mem_alloc(len, ALLOC_NOZERO) : nullptr;
    if (!data) {
        vfs_file_unref(file);
        return len ? ENOMEM : ENOEXEC;
    }

    kvec_t vec = {.iov_base = data, .iov_len = len};
    iovec_iter_t iter = iovec_iter_kernel(&vec, 1);
    size_t done = 0;
    while (!status && done < len) {
        ssize_t read = 0;
        status = vfs_file_pread(file, &iter, done, &read);
        if (!status && read <= 0)
            status = EIO;
        done += read > 0 ? read : 0;
    }
    vfs_file_unref(file);

    if (status) {
        mem_free(data, len);
        return status;
    }
    *out_data = data;
    *out_len = len;
    return 0;
}

struct syscall_ret sys_module_insert(const __user char* path, const __user char* cmdline) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    if (!ident_is_root(&proc->identity, false)) {
        ret.error = EPERM;
        return ret;
    }

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error)
        return ret;

    char* kcmdline = nullptr;
    if (cmdline)
        ret.error = copy_user_string(cmdline, MODULE_CMDLINE_MAX, &kcmdline);

    void* data = nullptr;
    size_t len = 0;
    if (!ret.error)
        ret.error = read_module_file(proc, kpath, &data, &len);

    if (!ret.error) {
        struct module* mod;
        ret.error = module_load(data, len, kcmdline ? kcmdline : "", &mod);
        if (ret.error)
            log_warn("module: Failed to load \"%s\" (%i)\n", kpath, ret.error);
    }

    mem_free(data, len);
    mem_free(kcmdline, MODULE_CMDLINE_MAX);
    mem_free(kpath, __PATH_MAX);
    return ret;
}

struct syscall_ret sys_module_remove(const __user char* name) {
    struct syscall_ret ret = {};

    if (!ident_is_root(&self()->identity, false)) {
        ret.error = EPERM;
        return ret;
    }

    char* kname;
    ret.error = copy_user_string(name, MODULE_NAME_MAX, &kname);
    if (ret.error)
        return ret;

    struct module* mod = module_find(kname);
    ret.error = mod ? module_unload(mod) : ENOENT;
    mem_free(kname, MODULE_NAME_MAX);
    return ret;
}
