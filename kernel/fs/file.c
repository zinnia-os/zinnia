// SPDX-License-Identifier: BSD-3-Clause

#include <fs/private.h>
#include <fs/vfs.h>
#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/print.h>
#include <mem/alloc.h>
#include <stdckdint.h>
#include <string.h>

errno_t vfs_file_new(struct vfs_node* node, const struct vfs_path* path, enum open_flags flags, struct vfs_file** out) {
    assert(node != nullptr, "Every open file must be backed by an inode");

    struct vfs_file* file = mem_alloc(sizeof(*file), 0);
    if (!file)
        return ENOMEM;

    refcount_init(&file->rc);
    file->ops = node->file_ops;
    file->vnode = node;
    vfs_node_ref(node);
    atomic_init(&file->flags, flags);
    mutex_init(&file->lock);

    switch (node->type) {
    case VNODE_REGULAR:
    case VNODE_DIRECTORY:
        file->position_kind = VFS_POSITION_ATOMIC;
        break;
    default:
        file->position_kind = VFS_POSITION_POSITION;
        break;
    }

    if (path) {
        file->path = vfs_path_clone(*path);
        file->has_path = true;
    }

    *out = file;
    return 0;
}

void vfs_stream_open(struct vfs_file* file) {
    file->position_kind = VFS_POSITION_STREAM;
}

// Resolves the absolute path of `path` as seen from `root` into a fitting allocation.
static char* make_absolute_path(struct vfs_path path, struct vfs_path* root) {
    char* buf = mem_alloc(__PATH_MAX, ALLOC_NOZERO);
    if (!buf)
        return nullptr;

    size_t len;
    char* result = nullptr;
    if (vfs_path_absolute(path, root, buf, __PATH_MAX, &len) == 0) {
        result = mem_alloc(len + 1, ALLOC_NOZERO);
        if (result)
            memcpy(result, buf, len + 1);
    }

    mem_free(buf, __PATH_MAX);
    return result;
}

static errno_t open_special(
    struct vfs_path* root,
    struct vfs_path* file_path,
    struct vfs_node* node,
    enum open_flags flags,
    struct vfs_file** out
) {
    if ((node->type == VNODE_CHARDEV || node->type == VNODE_BLOCKDEV) && (!node->ops || !node->ops->device.open))
        return ENXIO;

    struct vfs_file* file;
    errno_t status = vfs_file_new(node, file_path, flags, &file);
    if (status)
        return status;
    file->absolute_path = make_absolute_path(*file_path, root);

    status = node->type == VNODE_FIFO ? vfs_fifo_open(node, file) : node->ops->device.open(node, file);
    if (status) {
        file->ops = nullptr;
        vfs_file_unref(file);
        return status;
    }

    *out = file;
    return 0;
}

static enum access_flags open_access(enum open_flags flags) {
    if (flags & O_PATH)
        return F_OK;

    enum access_flags access = F_OK;
    if (flags & O_RDONLY)
        access |= R_OK;
    if (flags & (O_WRONLY | O_TRUNC))
        access |= W_OK;
    return access;
}

static errno_t do_open_inode(
    struct vfs_path* root,
    struct vfs_path* file_path,
    struct vfs_node* node,
    enum open_flags flags,
    bool created,
    const struct ident* identity,
    struct vfs_file** out
) {
    // If we want to open as a directory, make sure this is actually a directory.
    if ((flags & O_DIRECTORY) && node->type != VNODE_DIRECTORY)
        return ENOTDIR;

    if (node->type == VNODE_DIRECTORY && (flags & (O_WRONLY | O_CREAT | O_TRUNC)))
        return EISDIR;

    errno_t status = created ? 0 : vfs_node_try_access(node, identity, open_access(flags), false);
    if (status)
        return status;

    switch (node->type) {
    case VNODE_REGULAR: {
        if (flags & O_TRUNC) {
            if (!(flags & O_WRONLY))
                return EINVAL;
            if (!node->ops->regular.truncate)
                return EPERM;
            status = node->ops->regular.truncate(node, 0);
            if (status)
                return status;
        }

        struct vfs_file* file;
        status = vfs_file_new(node, file_path, flags, &file);
        if (status)
            return status;
        file->absolute_path = make_absolute_path(*file_path, root);
        *out = file;
        return 0;
    }
    case VNODE_DIRECTORY:
        if (node->ops->dir.open)
            return node->ops->dir.open(node, file_path, flags, identity, out);
        return vfs_file_new(node, file_path, flags, out);
    case VNODE_SYMLINK:
        return ELOOP;
    case VNODE_CHARDEV:
    case VNODE_BLOCKDEV:
    case VNODE_FIFO:
        return open_special(root, file_path, node, flags, out);
    case VNODE_SOCKET:
        return ENXIO;
    default:
        return ENOTSUP;
    }
}

errno_t vfs_file_open(
    struct vfs_path root,
    struct vfs_path cwd,
    const char* path,
    enum open_flags flags,
    mode_t mode,
    const struct ident* identity,
    struct vfs_file** out
) {
    if ((flags & O_DIRECTORY) && (flags & (O_CREAT | O_TMPFILE)))
        return EINVAL;

    const bool exclusive = (flags & (O_CREAT | O_EXCL)) == (O_CREAT | O_EXCL);

    enum vfs_lookup_flags lookup_flags = 0;
    // If we don't want to create a file when opening, the lookup must succeed.
    // If we want to create a temporary unnamed file, `path` is the directory of the owning FS.
    // In that case the path also has to exist.
    if (!(flags & O_CREAT) || (flags & O_TMPFILE))
        lookup_flags |= VFS_LOOKUP_MUST_EXIST;
    if (exclusive)
        lookup_flags |= VFS_LOOKUP_MUST_NOT_EXIST;
    if (!(flags & O_NOFOLLOW) && !exclusive)
        lookup_flags |= VFS_LOOKUP_FOLLOW_SYMLINKS;

    struct vfs_path file_path;
    errno_t status = vfs_path_lookup(root, cwd, path, identity, lookup_flags, &file_path);
    if (status)
        return status;

    bool created = false;
    struct vfs_node* node = vfs_entry_get_inode(file_path.entry);
    if (!node) {
        // If the lookup was successful, we expect that the entry is positive.
        if (!(flags & O_CREAT)) {
            log_warn("vfs: Tried opening a file without O_CREAT and backing inode, this is a bug!\n");
            status = ENOENT;
            goto out;
        }

        struct vfs_path parent_path;
        status = vfs_path_lookup_parent(file_path, &parent_path);
        if (status)
            goto out;
        struct vfs_node* parent = vfs_entry_get_inode(parent_path.entry);
        vfs_path_drop(parent_path);
        if (!parent) {
            status = ENOENT;
            goto out;
        }

        status = parent->type == VNODE_DIRECTORY ? 0 : ENOTDIR;
        if (!status)
            status = vfs_node_try_access(parent, identity, W_OK | X_OK, false);
        if (!status)
            status = parent->ops->dir.create ? parent->ops->dir.create(parent, file_path.entry, mode, identity) : EPERM;
        vfs_node_unref(parent);
        if (status)
            goto out;

        node = vfs_entry_get_inode(file_path.entry);
        if (!node) {
            status = EIO;
            goto out;
        }
        created = true;
    }

    status = do_open_inode(&root, &file_path, node, flags, created, identity, out);
    vfs_node_unref(node);

out:
    vfs_path_drop(file_path);
    return status;
}

static errno_t advance_offset(off_t* offset, ssize_t amount) {
    if (amount > 0 && ckd_add(offset, *offset, amount))
        return EOVERFLOW;
    return 0;
}

errno_t vfs_file_read(struct vfs_file* file, iovec_iter_t* buffer, ssize_t* out_read) {
    *out_read = 0;
    if (iovec_iter_len(buffer) == 0)
        return 0;
    if (!file->ops || !file->ops->read)
        return EINVAL;

    errno_t status;
    ssize_t read = 0;
    switch (file->position_kind) {
    case VFS_POSITION_STREAM:
        status = file->ops->read(file, buffer, 0, &read);
        break;
    case VFS_POSITION_ATOMIC:
        mutex_lock(&file->lock);
        status = file->ops->read(file, buffer, file->position, &read);
        if (!status)
            status = advance_offset(&file->position, read);
        mutex_unlock(&file->lock);
        break;
    case VFS_POSITION_POSITION: {
        mutex_lock(&file->lock);
        off_t pos = file->position;
        mutex_unlock(&file->lock);

        status = file->ops->read(file, buffer, pos, &read);
        if (!status)
            status = advance_offset(&pos, read);
        if (!status) {
            mutex_lock(&file->lock);
            file->position = pos;
            mutex_unlock(&file->lock);
        }
        break;
    }
    default:
        return EINVAL;
    }

    if (status)
        return status;
    *out_read = read;
    return 0;
}

errno_t vfs_file_pread(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    *out_read = 0;
    if (iovec_iter_len(buffer) == 0)
        return 0;
    if (file->position_kind == VFS_POSITION_STREAM)
        return ESPIPE;
    if (!file->ops || !file->ops->read)
        return EINVAL;

    return file->ops->read(file, buffer, offset, out_read);
}

static errno_t check_write_progress(ssize_t written) {
    if (written == 0) {
        log_warn("vfs: write reported zero progress for a non-empty buffer\n");
        return EIO;
    }
    return 0;
}

static errno_t sync_after_write(struct vfs_file* file, ssize_t written) {
    if (written <= 0)
        return 0;

    if ((atomic_load(&file->flags, ATOMIC_RELAXED) & O_SYNC) == O_SYNC)
        return vfs_file_sync(file, false);
    if ((atomic_load(&file->flags, ATOMIC_RELAXED) & O_DSYNC) == O_DSYNC)
        return vfs_file_sync(file, true);
    return 0;
}

errno_t vfs_file_write(struct vfs_file* file, iovec_iter_t* buffer, ssize_t* out_written) {
    *out_written = 0;
    if (iovec_iter_len(buffer) == 0)
        return 0;
    if (!file->ops || !file->ops->write)
        return EINVAL;

    // TODO: Appending writes through different files of the same inode aren't serialized.
    const bool append = atomic_load(&file->flags, ATOMIC_RELAXED) & O_APPEND;

    errno_t status;
    ssize_t written = 0;
    switch (file->position_kind) {
    case VFS_POSITION_STREAM:
        status = file->ops->write(file, buffer, 0, &written);
        break;
    case VFS_POSITION_ATOMIC:
        mutex_lock(&file->lock);
        if (append)
            file->position = file->vnode->size;
        status = file->ops->write(file, buffer, file->position, &written);
        if (!status)
            status = advance_offset(&file->position, written);
        mutex_unlock(&file->lock);
        break;
    case VFS_POSITION_POSITION: {
        mutex_lock(&file->lock);
        off_t pos = append ? (off_t)file->vnode->size : file->position;
        mutex_unlock(&file->lock);

        status = file->ops->write(file, buffer, pos, &written);
        if (!status)
            status = advance_offset(&pos, written);
        if (!status) {
            mutex_lock(&file->lock);
            file->position = pos;
            mutex_unlock(&file->lock);
        }
        break;
    }
    default:
        return EINVAL;
    }

    if (!status)
        status = check_write_progress(written);
    if (!status)
        status = sync_after_write(file, written);
    if (status)
        return status;

    *out_written = written;
    return 0;
}

errno_t vfs_file_pwrite(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written) {
    *out_written = 0;
    if (iovec_iter_len(buffer) == 0)
        return 0;
    if (file->position_kind == VFS_POSITION_STREAM)
        return ESPIPE;
    if (!file->ops || !file->ops->write)
        return EINVAL;

    ssize_t written = 0;
    errno_t status = file->ops->write(file, buffer, offset, &written);
    if (!status)
        status = check_write_progress(written);
    if (!status)
        status = sync_after_write(file, written);
    if (status)
        return status;

    *out_written = written;
    return 0;
}

errno_t vfs_file_ioctl(struct vfs_file* file, uintptr_t request, uintptr_t arg, uintptr_t* out_result) {
    if (!file->ops || !file->ops->ioctl)
        return ENOTTY;
    return file->ops->ioctl(file, request, arg, out_result);
}

errno_t vfs_file_sync(struct vfs_file* file, bool data_only) {
    if (!file->ops || !file->ops->sync)
        return 0;
    return file->ops->sync(file, data_only);
}

errno_t vfs_file_seek(struct vfs_file* file, enum vfs_seek_anchor anchor, off_t offset, off_t* out_position) {
    if (file->position_kind == VFS_POSITION_STREAM)
        return ESPIPE;

    errno_t status = 0;
    mutex_lock(&file->lock);

    off_t base;
    switch (anchor) {
    case VFS_SEEK_START:
        base = 0;
        break;
    case VFS_SEEK_CURRENT:
        base = file->position;
        break;
    case VFS_SEEK_END:
        base = file->vnode->size;
        break;
    default:
        status = EINVAL;
        goto out;
    }

    off_t new_position;
    if (ckd_add(&new_position, base, offset)) {
        status = EOVERFLOW;
        goto out;
    }
    if (new_position < 0) {
        status = EINVAL;
        goto out;
    }

    file->position = new_position;
    if (out_position)
        *out_position = new_position;

out:
    mutex_unlock(&file->lock);
    return status;
}

off_t vfs_file_tell(struct vfs_file* file) {
    mutex_lock(&file->lock);
    off_t position = file->position;
    mutex_unlock(&file->lock);
    return position;
}

errno_t vfs_file_mmap(
    struct vfs_file* file,
    struct vma* vma,
    uintptr_t addr,
    size_t len,
    enum prot_flags prot,
    enum map_flags flags,
    off_t offset,
    uintptr_t* out_addr
) {
    if (!file->ops || !file->ops->mmap)
        return ENODEV;
    return file->ops->mmap(file, vma, addr, len, prot, flags, offset, out_addr);
}

enum poll_flags vfs_poll(struct vfs_file* file, enum poll_flags events, struct poll_table* table) {
    const enum poll_flags always = POLLERR | POLLHUP | POLLNVAL;
    if (!file->ops || !file->ops->poll)
        return events & (POLLIN | POLLOUT | POLLRDNORM | POLLWRNORM);
    return file->ops->poll(file, events | POLLERR | POLLHUP, table) & (events | always);
}

errno_t vfs_file_get_object(struct vfs_file* file, struct vmo** out) {
    struct vfs_node* node = file->vnode;
    if (node->type != VNODE_REGULAR || !node->ops || !node->ops->regular.get_vmo)
        return ENODEV;
    return node->ops->regular.get_vmo(node, out);
}

static uint8_t dirent_type(enum vfs_node_type type) {
    switch (type) {
    case VNODE_REGULAR:
        return __DT_REG;
    case VNODE_DIRECTORY:
        return __DT_DIR;
    case VNODE_SYMLINK:
        return __DT_LNK;
    case VNODE_FIFO:
        return __DT_FIFO;
    case VNODE_BLOCKDEV:
        return __DT_BLK;
    case VNODE_CHARDEV:
        return __DT_CHR;
    case VNODE_SOCKET:
        return __DT_SOCK;
    default:
        // Should be unreachable, but just as a failsafe in case someone messes up.
        return __DT_UNKNOWN;
    }
}

errno_t vfs_dir_get_cached_entries(
    struct vfs_node* node,
    struct vfs_entry* entry,
    off_t offset,
    struct __dirent* buffer,
    size_t buffer_len,
    const struct ident* identity,
    size_t* out_count
) {
    size_t read = 0;
    off_t current = 0;

    mutex_lock(&entry->lock);
    for (struct vfs_entry* child = tailq_first(&entry->children); child && read < buffer_len;
         child = tailq_next(child, child_link)) {
        struct vfs_node* child_node = vfs_entry_get_inode(child);
        if (!child_node)
            continue;

        if (current < offset) {
            current++;
            vfs_node_unref(child_node);
            continue;
        }

        struct __dirent* d = &buffer[read];
        d->d_ino = child_node->id;
        d->d_off = current;
        d->d_reclen = sizeof(*d);
        d->d_type = dirent_type(child_node->type);
        memcpy(d->d_name, child->name, child->name_len);
        d->d_name[child->name_len] = '\0';
        vfs_node_unref(child_node);

        read++;
        current++;
    }
    mutex_unlock(&entry->lock);

    *out_count = read;
    return 0;
}

static void fill_dot_entry(struct __dirent* d, ino_t ino, off_t off, const char* name) {
    d->d_ino = ino;
    d->d_off = off;
    d->d_reclen = sizeof(*d);
    d->d_type = __DT_DIR;
    memset(d->d_name, 0, sizeof(d->d_name));
    memcpy(d->d_name, name, strlen(name));
}

errno_t vfs_get_dir_entries(
    struct vfs_file* file,
    struct __dirent* buffer,
    size_t buffer_len,
    const struct ident* identity,
    size_t* out_count
) {
    *out_count = 0;
    if (buffer_len == 0)
        return 0;

    struct vfs_node* node = file->vnode;
    if (!file->has_path || node->type != VNODE_DIRECTORY)
        return ENOTDIR;
    if (file->position_kind == VFS_POSITION_STREAM)
        return EBADF;

    ino_t parent_ino = node->id;
    struct vfs_path parent;
    if (vfs_path_lookup_parent(file->path, &parent) == 0) {
        struct vfs_node* parent_node = vfs_entry_get_inode(parent.entry);
        if (parent_node) {
            parent_ino = parent_node->id;
            vfs_node_unref(parent_node);
        }
        vfs_path_drop(parent);
    }

    errno_t status = 0;
    mutex_lock(&file->lock);

    const off_t start_offset = file->position;
    size_t num_read = 0;

    // Read `.` and `..` entries.
    if (start_offset == 0)
        fill_dot_entry(&buffer[num_read++], node->id, 0, ".");
    if ((start_offset == 1 || num_read == 1) && num_read < buffer_len)
        fill_dot_entry(&buffer[num_read++], parent_ino, 1, "..");

    // Read normal dir entries.
    if (num_read < buffer_len && node->ops->dir.get_dir_entries) {
        size_t count = 0;
        status = node->ops->dir.get_dir_entries(
            node,
            file->path.entry,
            start_offset > 2 ? start_offset - 2 : 0,
            buffer + num_read,
            buffer_len - num_read,
            identity,
            &count
        );
        if (status)
            goto out;
        num_read += count;
    }

    file->position += num_read;
    for (size_t i = 0; i < num_read; i++)
        buffer[i].d_off = start_offset + i + 1;

    *out_count = num_read;

out:
    mutex_unlock(&file->lock);
    return status;
}
