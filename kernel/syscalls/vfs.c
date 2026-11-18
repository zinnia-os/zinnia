// SPDX-License-Identifier: BSD-3-Clause

#include <dev/block/blockdev.h>
#include <fs/pipe.h>
#include <fs/poll.h>
#include <fs/vfs.h>
#include <lib/atomic.h>
#include <lib/clock.h>
#include <lib/iovec.h>
#include <lib/print.h>
#include <lib/time.h>
#include <mem/alloc.h>
#include <mem/usercopy.h>
#include <syscalls/private.h>
#include <stdckdint.h>
#include <zinnia/fcntl.h>
#include <zinnia/ioctls.h>
#include <zinnia/limits.h>
#include <zinnia/poll.h>
#include <zinnia/signal.h>
#include <zinnia/stat.h>
#include <zinnia/time.h>

static errno_t get_file(int fd, enum open_flags access, struct vfs_file** out) {
    struct vfs_file* file = process_fd_get(self(), fd);
    if (!file)
        return EBADF;
    if (!(atomic_load(&file->flags, ATOMIC_RELAXED) & access)) {
        vfs_file_unref(file);
        return EBADF;
    }
    *out = file;
    return 0;
}

static errno_t get_any_file(int fd, struct vfs_file** out) {
    struct vfs_file* file = process_fd_get(self(), fd);
    if (!file)
        return EBADF;
    *out = file;
    return 0;
}

static void set_status_flags(struct vfs_file* file, enum open_flags mask, enum open_flags value) {
    mutex_lock(&file->lock);
    atomic_store(&file->flags, (atomic_load(&file->flags, ATOMIC_RELAXED) & ~mask) | (value & mask), ATOMIC_RELAXED);
    mutex_unlock(&file->lock);
}

static void get_root_cwd(struct process* proc, struct vfs_path* out_root, struct vfs_path* out_cwd) {
    mutex_lock(&proc->lock);
    *out_root = vfs_path_clone(proc->root);
    if (out_cwd)
        *out_cwd = vfs_path_clone(proc->cwd);
    mutex_unlock(&proc->lock);
}

static errno_t get_at_start(
    struct process* proc,
    int dirfd,
    const char* path,
    struct vfs_path* out_root,
    struct vfs_path* out_start
) {
    struct vfs_path root, cwd;
    get_root_cwd(proc, &root, &cwd);

    if (dirfd != __AT_FDCWD && path[0] != '/') {
        struct vfs_file* dir = process_fd_get(proc, dirfd);
        errno_t status = !dir ? EBADF : !dir->has_path ? ENOTDIR : 0;
        if (!status) {
            vfs_path_drop(cwd);
            cwd = vfs_path_clone(dir->path);
        }
        if (dir)
            vfs_file_unref(dir);
        if (status) {
            vfs_path_drop(root);
            vfs_path_drop(cwd);
            return status;
        }
    }

    *out_root = root;
    *out_start = cwd;
    return 0;
}

static errno_t get_at_node(
    struct process* proc,
    int dirfd,
    const char* path,
    uint32_t at_flags,
    enum vfs_lookup_flags lookup_flags,
    struct vfs_node** out
) {
    if (path[0] == '\0' && (at_flags & __AT_EMPTY_PATH)) {
        struct vfs_node* node;
        if (dirfd == __AT_FDCWD) {
            mutex_lock(&proc->lock);
            node = vfs_entry_get_inode(proc->cwd.entry);
            mutex_unlock(&proc->lock);
        } else {
            struct vfs_file* file = process_fd_get(proc, dirfd);
            if (!file)
                return EBADF;
            node = file->vnode;
            vfs_node_ref(node);
            vfs_file_unref(file);
        }
        if (!node)
            return ENOENT;
        *out = node;
        return 0;
    }

    lookup_flags |= VFS_LOOKUP_MUST_EXIST;
    if (!(at_flags & __AT_SYMLINK_NOFOLLOW))
        lookup_flags |= VFS_LOOKUP_FOLLOW_SYMLINKS;

    struct vfs_path root, start;
    errno_t status = get_at_start(proc, dirfd, path, &root, &start);
    if (status)
        return status;

    struct vfs_path target;
    status = vfs_path_lookup(root, start, path, &proc->identity, lookup_flags, &target);
    vfs_path_drop(root);
    vfs_path_drop(start);
    if (status)
        return status;

    struct vfs_node* node = vfs_entry_get_inode(target.entry);
    vfs_path_drop(target);
    if (!node)
        return ENOENT;

    *out = node;
    return 0;
}

static errno_t lookup_dir(struct process* proc, const char* path, struct vfs_path* out) {
    struct vfs_path root, cwd;
    get_root_cwd(proc, &root, &cwd);

    struct vfs_path target;
    errno_t status =
        vfs_path_lookup(root, cwd, path, &proc->identity, VFS_LOOKUP_MUST_EXIST | VFS_LOOKUP_FOLLOW_SYMLINKS, &target);
    vfs_path_drop(root);
    vfs_path_drop(cwd);
    if (status)
        return status;

    struct vfs_node* node = vfs_entry_get_inode(target.entry);
    bool is_dir = node && node->type == VNODE_DIRECTORY;
    if (node)
        vfs_node_unref(node);
    if (!is_dir) {
        vfs_path_drop(target);
        return ENOTDIR;
    }

    *out = target;
    return 0;
}

static mode_t current_umask(struct process* proc) {
    mutex_lock(&proc->lock);
    mode_t mask = proc->umask;
    mutex_unlock(&proc->lock);
    return mask;
}

static void set_cwd(struct process* proc, struct vfs_path path) {
    mutex_lock(&proc->lock);
    struct vfs_path old = proc->cwd;
    proc->cwd = path;
    mutex_unlock(&proc->lock);
    vfs_path_drop(old);
}

static struct syscall_ret do_dup(int fd, int min_fd, bool cloexec) {
    struct syscall_ret ret = {};

    struct vfs_file* file;
    ret.error = get_any_file(fd, &file);
    if (ret.error)
        return ret;

    int new_fd;
    ret.error = process_fd_alloc_min(self(), file, min_fd, cloexec, &new_fd);
    vfs_file_unref(file);
    if (!ret.error)
        ret.value = new_fd;
    return ret;
}

static struct syscall_ret fcntl_getlk(int fd, uintptr_t arg) {
    struct syscall_ret ret = {};

    struct vfs_file* file;
    ret.error = get_any_file(fd, &file);
    if (ret.error)
        return ret;
    vfs_file_unref(file);

    struct __flock lock;
    if (!usercopy_read(&lock, (const __user void*)arg, sizeof(lock))) {
        ret.error = EFAULT;
        return ret;
    }
    lock.l_type = __F_UNLCK;
    lock.l_pid = 0;
    if (!usercopy_write((__user void*)arg, &lock, sizeof(lock)))
        ret.error = EFAULT;
    return ret;
}

constexpr size_t GETDENTS_MAX_BATCH = 32;

static struct syscall_ret do_two_paths(
    int old_fd,
    const __user char* old_path,
    int new_fd,
    const __user char* new_path,
    bool link,
    bool follow
) {
    struct syscall_ret ret = {};

    char* kold;
    ret.error = copy_user_path(old_path, &kold);
    if (ret.error)
        return ret;
    char* knew;
    ret.error = copy_user_path(new_path, &knew);
    if (ret.error) {
        mem_free(kold, __PATH_MAX);
        return ret;
    }

    struct process* proc = self();
    struct vfs_path root, old_start, new_root, new_start;
    ret.error = get_at_start(proc, old_fd, kold, &root, &old_start);
    if (ret.error)
        goto out;
    ret.error = get_at_start(proc, new_fd, knew, &new_root, &new_start);
    if (ret.error)
        goto out_old;

    if (link)
        ret.error = vfs_link(root, old_start, kold, new_start, knew, follow, &proc->identity);
    else
        ret.error = vfs_rename(root, old_start, kold, new_start, knew, &proc->identity);

    vfs_path_drop(new_root);
    vfs_path_drop(new_start);
out_old:
    vfs_path_drop(root);
    vfs_path_drop(old_start);
out:
    mem_free(kold, __PATH_MAX);
    mem_free(knew, __PATH_MAX);
    return ret;
}

static size_t poll_scan(struct __pollfd* fds, struct vfs_file** files, size_t nfds, struct poll_table* table) {
    size_t ready = 0;
    for (size_t i = 0; i < nfds; i++) {
        fds[i].revents = 0;
        if (fds[i].fd < 0)
            continue;

        if (!files[i])
            fds[i].revents = POLLNVAL;
        else
            fds[i].revents = vfs_poll(files[i], (uint16_t)fds[i].events, table);

        if (fds[i].revents)
            ready++;
    }
    return ready;
}

static errno_t read_poll_deadline(
    const __user struct __timespec* timeout,
    bool* out_nonblocking,
    uint64_t* out_deadline
) {
    struct __timespec ts;
    if (!usercopy_read(&ts, timeout, sizeof(ts)))
        return EFAULT;
    if (ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1'000'000'000)
        return EINVAL;

    uint64_t ns;
    if (ckd_mul(&ns, (uint64_t)ts.tv_sec, 1'000'000'000) || ckd_add(&ns, ns, (uint64_t)ts.tv_nsec))
        ns = UINT64_MAX;

    *out_nonblocking = ns == 0;
    if (ckd_add(out_deadline, clock_get_elapsed_ns(), ns))
        *out_deadline = UINT64_MAX;
    return 0;
}

static struct syscall_ret do_remove(int dirfd, const __user char* path, bool dir) {
    struct syscall_ret ret = {};

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error)
        return ret;

    struct process* proc = self();
    struct vfs_path root, start;
    ret.error = get_at_start(proc, dirfd, kpath, &root, &start);
    if (!ret.error) {
        if (dir)
            ret.error = vfs_rmdir(root, start, kpath, &proc->identity);
        else
            ret.error = vfs_unlink(root, start, kpath, &proc->identity);
        vfs_path_drop(root);
        vfs_path_drop(start);
    }
    mem_free(kpath, __PATH_MAX);
    return ret;
}

constexpr size_t IOV_MAX = 1024;

static errno_t copy_iovecs(const __user iovec_t* iov, size_t count, iovec_t** out) {
    if (count > IOV_MAX)
        return EINVAL;
    const size_t buf_size = MAX(count, 1) * sizeof(iovec_t);
    iovec_t* iovecs = mem_alloc(buf_size, 0);
    if (!iovecs)
        return ENOMEM;
    if (count && !usercopy_read(iovecs, iov, count * sizeof(iovec_t))) {
        mem_free(iovecs, buf_size);
        return EFAULT;
    }
    *out = iovecs;
    return 0;
}

static struct syscall_ret do_io(int fd, iovec_t* iovecs, size_t count, bool write, bool positioned, uint64_t offset) {
    struct syscall_ret ret = {};

    struct vfs_file* file;
    ret.error = get_file(fd, write ? O_WRONLY : O_RDONLY, &file);
    if (ret.error)
        return ret;

    iovec_iter_t iter = iovec_iter_user(iovecs, count);

    ssize_t done = 0;
    if (write)
        ret.error = positioned ? vfs_file_pwrite(file, &iter, offset, &done) : vfs_file_write(file, &iter, &done);
    else
        ret.error = positioned ? vfs_file_pread(file, &iter, offset, &done) : vfs_file_read(file, &iter, &done);
    vfs_file_unref(file);
    if (!ret.error)
        ret.value = done;
    return ret;
}

static struct syscall_ret do_iov(int fd, const __user iovec_t* iov, size_t count, bool write) {
    struct syscall_ret ret = {};

    iovec_t* iovecs;
    ret.error = copy_iovecs(iov, count, &iovecs);
    if (ret.error)
        return ret;
    ret = do_io(fd, iovecs, count, write, false, 0);
    mem_free(iovecs, MAX(count, 1) * sizeof(iovec_t));
    return ret;
}

static struct syscall_ret do_sync(int fd, bool data_only) {
    struct syscall_ret ret = {};

    struct vfs_file* file;
    ret.error = get_any_file(fd, &file);
    if (ret.error)
        return ret;

    ret.error = vfs_file_sync(file, data_only);
    vfs_file_unref(file);
    return ret;
}

struct syscall_ret sys_pread(int fd, __user void* buf, size_t len, uint64_t offset) {
    iovec_t iov = {.iov_base = buf, .iov_len = len};
    return do_io(fd, &iov, 1, false, true, offset);
}

struct syscall_ret sys_readv(int fd, const __user iovec_t* iov, size_t count) {
    return do_iov(fd, iov, count, false);
}

struct syscall_ret sys_pwrite(int fd, const __user void* buf, size_t len, uint64_t offset) {
    iovec_t iov = {.iov_base = (__user void*)buf, .iov_len = len};
    return do_io(fd, &iov, 1, true, true, offset);
}

struct syscall_ret sys_writev(int fd, const __user iovec_t* iov, size_t count) {
    return do_iov(fd, iov, count, true);
}

struct syscall_ret sys_openat(int dirfd, const __user char* path, enum open_flags flags, mode_t mode) {
    struct syscall_ret ret = {};

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error)
        return ret;

    struct process* proc = self();
    struct vfs_path root, start;
    ret.error = get_at_start(proc, dirfd, kpath, &root, &start);
    if (ret.error) {
        mem_free(kpath, __PATH_MAX);
        return ret;
    }

    mutex_lock(&proc->lock);
    mode &= ~proc->umask;
    mutex_unlock(&proc->lock);

    struct vfs_file* file = nullptr;
    ret.error = vfs_file_open(root, start, kpath, flags & ~O_CLOEXEC, mode, &proc->identity, &file);
    vfs_path_drop(root);
    vfs_path_drop(start);
    mem_free(kpath, __PATH_MAX);
    if (ret.error)
        return ret;

    int fd;
    ret.error = process_fd_alloc(proc, file, flags & O_CLOEXEC, &fd);
    vfs_file_unref(file);
    if (!ret.error)
        ret.value = fd;
    return ret;
}

struct syscall_ret sys_seek(int fd, off_t offset, int whence) {
    struct syscall_ret ret = {};

    enum vfs_seek_anchor anchor;
    switch (whence) {
    case __SEEK_SET:
        anchor = VFS_SEEK_START;
        break;
    case __SEEK_CUR:
        anchor = VFS_SEEK_CURRENT;
        break;
    case __SEEK_END:
        anchor = VFS_SEEK_END;
        break;
    default:
        ret.error = EINVAL;
        return ret;
    }

    struct vfs_file* file;
    ret.error = get_any_file(fd, &file);
    if (ret.error)
        return ret;

    off_t position = 0;
    ret.error = vfs_file_seek(file, anchor, offset, &position);
    vfs_file_unref(file);
    if (!ret.error)
        ret.value = position;
    return ret;
}

struct syscall_ret sys_close(int fd) {
    struct syscall_ret ret = {};

    ret.error = process_fd_close(self(), fd);
    return ret;
}

struct syscall_ret sys_sync() {
    struct syscall_ret ret = {};

    ret.error = vfs_sync_all();
    errno_t block_status = blockdev_sync_all();
    if (!ret.error)
        ret.error = block_status;
    return ret;
}

struct syscall_ret sys_fsync(int fd) {
    return do_sync(fd, false);
}

struct syscall_ret sys_fdatasync(int fd) {
    return do_sync(fd, true);
}

struct syscall_ret sys_ioctl(int fd, uintptr_t request, uintptr_t arg) {
    struct syscall_ret ret = {};

    struct vfs_file* file;
    ret.error = get_any_file(fd, &file);
    if (ret.error)
        return ret;

    uintptr_t value = 0;
    if ((uint32_t)request == FIONBIO) {
        uint32_t nonblock;
        if (usercopy_read(&nonblock, (const __user void*)arg, sizeof(nonblock)))
            set_status_flags(file, O_NONBLOCK, nonblock ? O_NONBLOCK : 0);
        else
            ret.error = EFAULT;
    } else {
        ret.error = vfs_file_ioctl(file, request, arg, &value);
    }
    vfs_file_unref(file);
    if (!ret.error)
        ret.value = value;
    return ret;
}

struct syscall_ret sys_getcwd(__user char* buf, size_t len) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    struct vfs_path root, cwd;
    get_root_cwd(proc, &root, &cwd);

    char* kbuf = mem_alloc(__PATH_MAX, ALLOC_NOZERO);
    if (!kbuf)
        ret.error = ENOMEM;
    size_t path_len = 0;
    if (!ret.error)
        ret.error = vfs_path_absolute(cwd, &root, kbuf, __PATH_MAX, &path_len);
    vfs_path_drop(root);
    vfs_path_drop(cwd);

    if (!ret.error && path_len + 1 > len)
        ret.error = ERANGE;
    if (!ret.error && !usercopy_write(buf, kbuf, path_len + 1))
        ret.error = EFAULT;
    mem_free(kbuf, __PATH_MAX);
    if (!ret.error)
        ret.value = path_len + 1;
    return ret;
}

struct syscall_ret sys_fstat(int fd, __user struct __stat* buf) {
    struct syscall_ret ret = {};

    struct vfs_file* file;
    ret.error = get_any_file(fd, &file);
    if (ret.error)
        return ret;

    struct __stat st;
    vfs_stat(file->vnode, &st);
    vfs_file_unref(file);
    if (!usercopy_write(buf, &st, sizeof(st)))
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_fstatat(int dirfd, const __user char* path, __user struct __stat* buf, uint32_t flags) {
    struct syscall_ret ret = {};

    if (flags & ~(__AT_SYMLINK_NOFOLLOW | __AT_EMPTY_PATH)) {
        ret.error = EINVAL;
        return ret;
    }

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error)
        return ret;

    struct vfs_node* node;
    ret.error = get_at_node(self(), dirfd, kpath, flags, 0, &node);
    mem_free(kpath, __PATH_MAX);
    if (ret.error)
        return ret;

    struct __stat st;
    vfs_stat(node, &st);
    vfs_node_unref(node);
    if (!usercopy_write(buf, &st, sizeof(st)))
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_dup(int fd) {
    return do_dup(fd, 0, false);
}

struct syscall_ret sys_flock(int fd, int operation) {
    struct syscall_ret ret = {};

    struct vfs_file* file;
    ret.error = get_any_file(fd, &file);
    if (!ret.error)
        vfs_file_unref(file);
    return ret;
}

struct syscall_ret sys_dup3(int old_fd, int new_fd, uint32_t flags) {
    struct syscall_ret ret = {};

    if (flags & ~O_CLOEXEC) {
        ret.error = EINVAL;
        return ret;
    }
    if (old_fd == new_fd) {
        bool cloexec;
        ret.error = process_fd_get_cloexec(self(), old_fd, &cloexec);
        if (!ret.error)
            ret.value = old_fd;
        return ret;
    }

    struct vfs_file* file;
    ret.error = get_any_file(old_fd, &file);
    if (ret.error)
        return ret;

    ret.error = process_fd_replace(self(), new_fd, file, flags & O_CLOEXEC);
    vfs_file_unref(file);
    if (!ret.error)
        ret.value = new_fd;
    return ret;
}

struct syscall_ret sys_mknodat(int dirfd, const __user char* path, mode_t mode, dev_t dev) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    struct vfs_mknod_target target = {};
    switch (mode & __S_IFMT) {
    case __S_IFIFO:
        target.type = VNODE_FIFO;
        break;
    case __S_IFSOCK:
        target.type = VNODE_SOCKET;
        break;
    case __S_IFCHR:
    case __S_IFBLK:
        if (!ident_is_root(&proc->identity, false)) {
            ret.error = EPERM;
            return ret;
        }
        target.type = (mode & __S_IFMT) == __S_IFCHR ? VNODE_CHARDEV : VNODE_BLOCKDEV;
        target.rdev = dev;
        break;
    default:
        ret.error = ENOTSUP;
        return ret;
    }

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error)
        return ret;

    struct vfs_path root, start;
    ret.error = get_at_start(proc, dirfd, kpath, &root, &start);
    if (!ret.error) {
        ret.error =
            vfs_mknod(root, start, kpath, mode & ~current_umask(proc) & 07777, &target, &proc->identity, nullptr);
        vfs_path_drop(root);
        vfs_path_drop(start);
    }
    mem_free(kpath, __PATH_MAX);
    return ret;
}

struct syscall_ret sys_mkdirat(int dirfd, const __user char* path, mode_t mode) {
    struct syscall_ret ret = {};

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error)
        return ret;

    struct process* proc = self();
    struct vfs_path root, start;
    ret.error = get_at_start(proc, dirfd, kpath, &root, &start);
    if (!ret.error) {
        ret.error = vfs_mkdir(root, start, kpath, mode & ~current_umask(proc) & 07777, &proc->identity);
        vfs_path_drop(root);
        vfs_path_drop(start);
    }
    mem_free(kpath, __PATH_MAX);
    return ret;
}

struct syscall_ret sys_chdir(const __user char* path) {
    struct syscall_ret ret = {};

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error)
        return ret;

    struct process* proc = self();
    struct vfs_path target;
    ret.error = lookup_dir(proc, kpath, &target);
    mem_free(kpath, __PATH_MAX);
    if (ret.error)
        return ret;

    set_cwd(proc, target);
    return ret;
}

struct syscall_ret sys_fchdir(int fd) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    struct vfs_file* file = process_fd_get(proc, fd);
    if (!file) {
        ret.error = EBADF;
        return ret;
    }

    if (file->has_path && file->vnode->type == VNODE_DIRECTORY)
        set_cwd(proc, vfs_path_clone(file->path));
    else
        ret.error = ENOTDIR;
    vfs_file_unref(file);
    return ret;
}

struct syscall_ret sys_getdents(int fd, __user struct __dirent* buf, size_t len) {
    struct syscall_ret ret = {};

    size_t count = MIN(len / sizeof(struct __dirent), GETDENTS_MAX_BATCH);
    if (count == 0) {
        ret.error = EINVAL;
        return ret;
    }

    struct vfs_file* file;
    ret.error = get_file(fd, O_RDONLY, &file);
    if (ret.error)
        return ret;

    struct __dirent* entries = mem_alloc(count * sizeof(*entries), 0);
    if (!entries) {
        vfs_file_unref(file);
        ret.error = ENOMEM;
        return ret;
    }

    size_t read = 0;
    ret.error = vfs_get_dir_entries(file, entries, count, &self()->identity, &read);
    vfs_file_unref(file);
    if (!ret.error && read && !usercopy_write(buf, entries, read * sizeof(*entries)))
        ret.error = EFAULT;
    mem_free(entries, count * sizeof(*entries));
    if (!ret.error)
        ret.value = read * sizeof(struct __dirent);
    return ret;
}

struct syscall_ret sys_fcntl(int fd, int cmd, uintptr_t arg) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    struct vfs_file* file;

    switch (cmd) {
    case __F_DUPFD:
        return do_dup(fd, arg, false);
    case __F_DUPFD_CLOEXEC:
        return do_dup(fd, arg, true);
    case __F_GETFD: {
        bool cloexec;
        ret.error = process_fd_get_cloexec(proc, fd, &cloexec);
        if (!ret.error)
            ret.value = cloexec ? __FD_CLOEXEC : 0;
        return ret;
    }
    case __F_SETFD:
        ret.error = process_fd_set_cloexec(proc, fd, arg & __FD_CLOEXEC);
        return ret;
    case __F_GETFL:
        ret.error = get_any_file(fd, &file);
        if (ret.error)
            return ret;
        mutex_lock(&file->lock);
        ret.value = atomic_load(&file->flags, ATOMIC_RELAXED);
        mutex_unlock(&file->lock);
        vfs_file_unref(file);
        return ret;
    case __F_SETFL:
        ret.error = get_any_file(fd, &file);
        if (ret.error)
            return ret;
        set_status_flags(file, O_NONBLOCK | O_APPEND, arg);
        vfs_file_unref(file);
        return ret;
    case __F_GETOWN:
    case __F_SETOWN:
    case __F_GETOWN_EX:
    case __F_SETOWN_EX:
        log_warn("fcntl: Command %d is a stub\n", cmd);
        return ret;
    case __F_GETLK:
    case __F_OFD_GETLK:
        return fcntl_getlk(fd, arg);
    case __F_SETLK:
    case __F_SETLKW:
    case __F_OFD_SETLK:
    case __F_OFD_SETLKW:
        ret.error = get_any_file(fd, &file);
        if (!ret.error)
            vfs_file_unref(file);
        return ret;
    default:
        ret.error = EINVAL;
        return ret;
    }
}

struct syscall_ret sys_ppoll(
    __user struct __pollfd* user_fds,
    size_t nfds,
    const __user struct __timespec* timeout,
    const __user __sigset_t* sigmask
) {
    struct syscall_ret ret = {};

    if (nfds > process_fd_limit(self())) {
        ret.error = EINVAL;
        return ret;
    }

    bool nonblocking = false;
    uint64_t deadline = 0;
    if (timeout) {
        ret.error = read_poll_deadline(timeout, &nonblocking, &deadline);
        if (ret.error)
            return ret;
    }

    struct __pollfd* fds = mem_alloc(MAX(nfds, 1) * sizeof(*fds), 0);
    struct vfs_file** files = mem_alloc(MAX(nfds, 1) * sizeof(*files), 0);
    if (!fds || !files) {
        mem_free(fds, MAX(nfds, 1) * sizeof(*fds));
        mem_free(files, MAX(nfds, 1) * sizeof(*files));
        ret.error = ENOMEM;
        return ret;
    }
    if (nfds && !usercopy_read(fds, user_fds, nfds * sizeof(*fds))) {
        mem_free(fds, MAX(nfds, 1) * sizeof(*fds));
        mem_free(files, MAX(nfds, 1) * sizeof(*files));
        ret.error = EFAULT;
        return ret;
    }

    struct process* proc = self();
    for (size_t i = 0; i < nfds; i++)
        files[i] = fds[i].fd >= 0 ? process_fd_get(proc, fds[i].fd) : nullptr;

    struct poll_table table;
    poll_table_init(&table);

    size_t ready = 0;
    bool registered = false;
    while (true) {
        atomic_store(&table.triggered, false, ATOMIC_RELAXED);
        ready = poll_scan(fds, files, nfds, nonblocking || registered ? nullptr : &table);
        registered = true;

        if (table.error) {
            ret.error = table.error;
            break;
        }
        if (ready || nonblocking || (timeout && clock_get_elapsed_ns() >= deadline))
            break;

        if (poll_table_wait(&table, timeout ? &deadline : nullptr) == EINTR) {
            ret.error = EINTR;
            break;
        }
    }

    poll_table_fini(&table);
    for (size_t i = 0; i < nfds; i++) {
        if (files[i])
            vfs_file_unref(files[i]);
    }

    if (!ret.error && nfds && !usercopy_write(user_fds, fds, nfds * sizeof(*fds)))
        ret.error = EFAULT;
    mem_free(fds, MAX(nfds, 1) * sizeof(*fds));
    mem_free(files, MAX(nfds, 1) * sizeof(*files));
    if (!ret.error)
        ret.value = ready;
    return ret;
}

struct syscall_ret sys_pipe(__user int* fds, uint32_t flags) {
    struct syscall_ret ret = {};

    if (flags & ~(O_CLOEXEC | O_NONBLOCK)) {
        ret.error = EINVAL;
        return ret;
    }

    struct process* proc = self();
    struct vfs_file* read_end;
    struct vfs_file* write_end;
    ret.error = pipe_create(flags, &proc->identity, &read_end, &write_end);
    if (ret.error)
        return ret;

    const bool cloexec = flags & O_CLOEXEC;
    int kfds[2] = {-1, -1};
    ret.error = process_fd_alloc(proc, read_end, cloexec, &kfds[0]);
    if (!ret.error)
        ret.error = process_fd_alloc(proc, write_end, cloexec, &kfds[1]);
    if (!ret.error && !usercopy_write(fds, kfds, sizeof(kfds)))
        ret.error = EFAULT;

    if (ret.error) {
        for (size_t i = 0; i < 2; i++) {
            if (kfds[i] >= 0)
                process_fd_close(proc, kfds[i]);
        }
    }
    vfs_file_unref(read_end);
    vfs_file_unref(write_end);
    return ret;
}

struct syscall_ret sys_faccessat(int dirfd, const __user char* path, uint32_t amode, uint32_t flags) {
    struct syscall_ret ret = {};

    if (amode & ~(__R_OK | __W_OK | __X_OK)) {
        ret.error = EINVAL;
        return ret;
    }
    if (flags & ~(__AT_EACCESS | __AT_SYMLINK_NOFOLLOW | __AT_EMPTY_PATH)) {
        ret.error = EINVAL;
        return ret;
    }

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error)
        return ret;

    struct process* proc = self();
    const bool use_real = !(flags & __AT_EACCESS);
    struct vfs_node* node;
    ret.error = get_at_node(proc, dirfd, kpath, flags, use_real ? VFS_LOOKUP_REAL_ID : 0, &node);
    mem_free(kpath, __PATH_MAX);
    if (ret.error)
        return ret;

    ret.error = vfs_node_try_access(node, &proc->identity, amode, use_real);
    vfs_node_unref(node);
    return ret;
}

struct syscall_ret sys_statvfs(const __user char* path, __user struct __statvfs* buf) {
    struct syscall_ret ret = {};

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error)
        return ret;

    struct vfs_node* node;
    ret.error = get_at_node(self(), __AT_FDCWD, kpath, 0, 0, &node);
    mem_free(kpath, __PATH_MAX);
    if (ret.error)
        return ret;

    struct __statvfs st;
    ret.error = vfs_statvfs(node, &st);
    vfs_node_unref(node);
    if (ret.error)
        return ret;
    if (!usercopy_write(buf, &st, sizeof(st)))
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_fstatvfs(int fd, __user struct __statvfs* buf) {
    struct syscall_ret ret = {};

    struct vfs_file* file;
    ret.error = get_any_file(fd, &file);
    if (ret.error)
        return ret;

    struct __statvfs st;
    ret.error = vfs_statvfs(file->vnode, &st);
    vfs_file_unref(file);
    if (ret.error)
        return ret;
    if (!usercopy_write(buf, &st, sizeof(st)))
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_renameat(int old_fd, const __user char* old_path, int new_fd, const __user char* new_path) {
    return do_two_paths(old_fd, old_path, new_fd, new_path, false, false);
}

struct syscall_ret sys_fchmod(int fd, mode_t mode) {
    struct syscall_ret ret = {};

    struct vfs_file* file;
    ret.error = get_any_file(fd, &file);
    if (ret.error)
        return ret;

    ret.error = vfs_chmod(file->vnode, mode, &self()->identity);
    vfs_file_unref(file);
    return ret;
}

struct syscall_ret sys_truncate(const __user char* path, off_t length) {
    struct syscall_ret ret = {};

    if (length < 0) {
        ret.error = EINVAL;
        return ret;
    }

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error)
        return ret;

    struct process* proc = self();
    struct vfs_node* node;
    ret.error = get_at_node(proc, __AT_FDCWD, kpath, 0, 0, &node);
    mem_free(kpath, __PATH_MAX);
    if (ret.error)
        return ret;

    ret.error = vfs_node_try_access(node, &proc->identity, W_OK, false);
    if (!ret.error)
        ret.error = vfs_truncate(node, length);
    vfs_node_unref(node);
    return ret;
}

struct syscall_ret sys_ftruncate(int fd, off_t length) {
    struct syscall_ret ret = {};

    if (length < 0) {
        ret.error = EINVAL;
        return ret;
    }

    struct vfs_file* file;
    ret.error = get_any_file(fd, &file);
    if (ret.error)
        return ret;

    ret.error = (atomic_load(&file->flags, ATOMIC_RELAXED) & O_WRONLY) ? vfs_truncate(file->vnode, length) : EINVAL;
    vfs_file_unref(file);
    return ret;
}

struct syscall_ret sys_fallocate(int fd, off_t offset, off_t length) {
    struct syscall_ret ret = {};

    if (offset < 0 || length <= 0) {
        ret.error = EINVAL;
        return ret;
    }

    struct vfs_file* file;
    ret.error = get_file(fd, O_WRONLY, &file);
    if (ret.error)
        return ret;

    struct vfs_node* node = file->vnode;
    if (node->type != VNODE_REGULAR) {
        vfs_file_unref(file);
        ret.error = ENODEV;
        return ret;
    }

    uint64_t required;
    if (ckd_add(&required, (uint64_t)offset, (uint64_t)length))
        required = UINT64_MAX;

    mutex_lock(&node->lock);
    const bool grow = required > node->size;
    mutex_unlock(&node->lock);

    if (grow)
        ret.error = vfs_truncate(node, required);
    vfs_file_unref(file);
    return ret;
}

struct syscall_ret sys_fchmodat(int dirfd, const __user char* path, mode_t mode, uint32_t flags) {
    struct syscall_ret ret = {};

    if (flags & ~(__AT_SYMLINK_NOFOLLOW | __AT_EMPTY_PATH)) {
        ret.error = EINVAL;
        return ret;
    }

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error)
        return ret;

    struct process* proc = self();
    struct vfs_node* node;
    ret.error = get_at_node(proc, dirfd, kpath, flags, 0, &node);
    mem_free(kpath, __PATH_MAX);
    if (ret.error)
        return ret;

    ret.error = vfs_chmod(node, mode, &proc->identity);
    vfs_node_unref(node);
    return ret;
}

struct syscall_ret sys_fchownat(int dirfd, const __user char* path, uid_t uid, gid_t gid, uint32_t flags) {
    struct syscall_ret ret = {};

    if (flags & ~(__AT_SYMLINK_NOFOLLOW | __AT_EMPTY_PATH)) {
        ret.error = EINVAL;
        return ret;
    }

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error)
        return ret;

    struct process* proc = self();
    struct vfs_node* node;
    ret.error = get_at_node(proc, dirfd, kpath, flags, 0, &node);
    mem_free(kpath, __PATH_MAX);
    if (ret.error)
        return ret;

    ret.error = vfs_chown(node, uid, gid, &proc->identity);
    vfs_node_unref(node);
    return ret;
}

struct syscall_ret sys_utimensat(int dirfd, const __user char* path, const __user timespec_t* times, uint32_t flags) {
    struct syscall_ret ret = {};

    if (flags & ~(__AT_SYMLINK_NOFOLLOW | __AT_EMPTY_PATH)) {
        ret.error = EINVAL;
        return ret;
    }

    timespec_t ts[2];
    if (times && !usercopy_read(ts, times, sizeof(ts))) {
        ret.error = EFAULT;
        return ret;
    }

    char* kpath = nullptr;
    if (path) {
        ret.error = copy_user_path(path, &kpath);
        if (ret.error)
            return ret;
    } else {
        flags |= __AT_EMPTY_PATH;
    }

    struct process* proc = self();
    struct vfs_node* node;
    ret.error = get_at_node(proc, dirfd, kpath ? kpath : "", flags, 0, &node);
    mem_free(kpath, __PATH_MAX);
    if (ret.error)
        return ret;

    ret.error = vfs_utimens(node, times ? ts : nullptr, &proc->identity);
    vfs_node_unref(node);
    return ret;
}

struct syscall_ret sys_unlinkat(int dirfd, const __user char* path, uint32_t flags) {
    struct syscall_ret ret = {};

    if (flags & ~__AT_REMOVEDIR) {
        ret.error = EINVAL;
        return ret;
    }
    return do_remove(dirfd, path, flags & __AT_REMOVEDIR);
}

struct syscall_ret sys_rmdirat(int dirfd, const __user char* path) {
    return do_remove(dirfd, path, true);
}

struct syscall_ret sys_linkat(
    int old_fd,
    const __user char* old_path,
    int new_fd,
    const __user char* new_path,
    uint32_t flags
) {
    struct syscall_ret ret = {};

    if (flags & ~__AT_SYMLINK_FOLLOW) {
        ret.error = EINVAL;
        return ret;
    }
    return do_two_paths(old_fd, old_path, new_fd, new_path, true, flags & __AT_SYMLINK_FOLLOW);
}

struct syscall_ret sys_symlinkat(const __user char* target, int dirfd, const __user char* path) {
    struct syscall_ret ret = {};

    char* ktarget;
    ret.error = copy_user_path(target, &ktarget);
    if (ret.error)
        return ret;
    if (ktarget[0] == '\0') {
        mem_free(ktarget, __PATH_MAX);
        ret.error = ENOENT;
        return ret;
    }

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error) {
        mem_free(ktarget, __PATH_MAX);
        return ret;
    }

    struct process* proc = self();
    struct vfs_path root, start;
    ret.error = get_at_start(proc, dirfd, kpath, &root, &start);
    if (!ret.error) {
        ret.error = vfs_symlink(root, start, kpath, ktarget, &proc->identity);
        vfs_path_drop(root);
        vfs_path_drop(start);
    }
    mem_free(kpath, __PATH_MAX);
    mem_free(ktarget, __PATH_MAX);
    return ret;
}

struct syscall_ret sys_readlinkat(int dirfd, const __user char* path, __user char* buf, size_t len) {
    struct syscall_ret ret = {};

    char* kpath;
    ret.error = copy_user_path(path, &kpath);
    if (ret.error)
        return ret;

    struct vfs_node* node;
    ret.error = get_at_node(self(), dirfd, kpath, __AT_SYMLINK_NOFOLLOW, 0, &node);
    mem_free(kpath, __PATH_MAX);
    if (ret.error)
        return ret;

    iovec_t iov = {.iov_base = buf, .iov_len = len};
    iovec_iter_t iter = iovec_iter_user(&iov, 1);

    size_t read = 0;
    ret.error = vfs_read_link(node, &iter, &read);
    vfs_node_unref(node);
    if (!ret.error)
        ret.value = read;
    return ret;
}

struct syscall_ret sys_mount(
    const __user char* type,
    const __user char* dir,
    enum mount_flags flags,
    const __user void* data
) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    if (!ident_is_root(&proc->identity, false)) {
        ret.error = EPERM;
        return ret;
    }

    char* ktype;
    ret.error = copy_user_path(type, &ktype);
    if (ret.error)
        return ret;
    char* kdir;
    ret.error = copy_user_path(dir, &kdir);
    if (ret.error) {
        mem_free(ktype, __PATH_MAX);
        return ret;
    }

    char* kdata = nullptr;
    if (data) {
        ret.error = copy_user_path(data, &kdata);
        if (ret.error)
            goto out;
    }

    struct vfs_path target;
    ret.error = lookup_dir(proc, kdir, &target);
    if (ret.error)
        goto out;

    struct vfs_mount* mount;
    ret.error = vfs_mount(ktype, flags, kdata, &mount);
    if (!ret.error) {
        ret.error = vfs_path_mount(target, mount);
        if (ret.error)
            vfs_unmount(mount);
        vfs_mount_unref(mount);
    }
    vfs_path_drop(target);

out:
    mem_free(kdata, __PATH_MAX);
    mem_free(kdir, __PATH_MAX);
    mem_free(ktype, __PATH_MAX);
    return ret;
}

struct syscall_ret sys_chroot(const __user char* path) {
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

    struct vfs_path target;
    ret.error = lookup_dir(proc, kpath, &target);
    mem_free(kpath, __PATH_MAX);
    if (ret.error)
        return ret;

    mutex_lock(&proc->lock);
    struct vfs_path old = proc->root;
    proc->root = target;
    mutex_unlock(&proc->lock);
    vfs_path_drop(old);
    return ret;
}

struct syscall_ret sys_umount(const __user char* dir, uint32_t flags) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    if (!ident_is_root(&proc->identity, false)) {
        ret.error = EPERM;
        return ret;
    }

    char* kdir;
    ret.error = copy_user_path(dir, &kdir);
    if (ret.error)
        return ret;

    struct vfs_path target;
    ret.error = lookup_dir(proc, kdir, &target);
    mem_free(kdir, __PATH_MAX);
    if (ret.error)
        return ret;

    ret.error = vfs_path_unmount(target);
    vfs_path_drop(target);
    return ret;
}

struct syscall_ret sys_fadvise(int fd, off_t offset, off_t length, int advice) {
    struct syscall_ret ret = {};

    if (length < 0 || advice < __POSIX_FADV_NORMAL || advice > __POSIX_FADV_NOREUSE) {
        ret.error = EINVAL;
        return ret;
    }

    struct vfs_file* file;
    ret.error = get_any_file(fd, &file);
    if (ret.error)
        return ret;

    if (file->vnode->type == VNODE_FIFO)
        ret.error = ESPIPE;
    vfs_file_unref(file);
    return ret;
}
