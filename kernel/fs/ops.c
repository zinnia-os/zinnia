// SPDX-License-Identifier: BSD-3-Clause

#include <fs/vfs.h>
#include <lib/clock.h>
#include <string.h>

static bool is_read_only(struct vfs_node* node) {
    return node->sb && (node->sb->flags & MNT_RDONLY);
}

errno_t vfs_node_try_access(
    struct vfs_node* node,
    const struct ident* identity,
    enum access_flags access,
    bool use_real
) {
    mutex_lock(&node->lock);
    const mode_t mode = node->mode;
    const uid_t owner = node->uid;
    const gid_t group = node->gid;
    mutex_unlock(&node->lock);

    if ((access & W_OK) && is_read_only(node) &&
        (node->type == VNODE_REGULAR || node->type == VNODE_DIRECTORY || node->type == VNODE_SYMLINK))
        return EROFS;
    if ((access & X_OK) && node->type == VNODE_REGULAR && node->sb && (node->sb->flags & MNT_NOEXEC))
        return EACCES;

    if (ident_is_root(identity, use_real)) {
        if ((access & X_OK) && node->type != VNODE_DIRECTORY && !(mode & (__S_IXUSR | __S_IXGRP | __S_IXOTH)))
            return EACCES;
        return 0;
    }

    mode_t granted;
    if ((use_real ? identity->uid : identity->euid) == owner)
        granted = (mode >> 6) & 7;
    else if (ident_in_group(identity, group, use_real))
        granted = (mode >> 3) & 7;
    else
        granted = mode & 7;

    return (access & (R_OK | W_OK | X_OK) & ~granted) ? EACCES : 0;
}

static errno_t sync_node(struct vfs_node* node) {
    return node->ops && node->ops->sync ? node->ops->sync(node) : 0;
}

static timespec_t now() {
    return time_from_nano(clock_get_realtime_ns());
}

// Looks up the parent directory of `path` and checks that `identity` may write to it.
// Returns a new reference to the parent inode in `out`.
static errno_t get_writable_parent(struct vfs_path path, const struct ident* identity, struct vfs_node** out) {
    struct vfs_path parent;
    errno_t status = vfs_path_lookup_parent(path, &parent);
    if (status)
        return status;

    struct vfs_node* node = vfs_entry_get_inode(parent.entry);
    vfs_path_drop(parent);
    if (!node)
        return ENOENT;

    if (node->type != VNODE_DIRECTORY) {
        vfs_node_unref(node);
        return ENOTDIR;
    }

    status = vfs_node_try_access(node, identity, W_OK | X_OK, false);
    if (status) {
        vfs_node_unref(node);
        return status;
    }

    *out = node;
    return 0;
}

errno_t vfs_mkdir(
    struct vfs_path root,
    struct vfs_path cwd,
    const char* path,
    mode_t mode,
    const struct ident* identity
) {
    struct vfs_path target;
    errno_t status = vfs_path_lookup(root, cwd, path, identity, VFS_LOOKUP_MUST_NOT_EXIST, &target);
    if (status)
        return status;

    struct vfs_node* parent;
    status = get_writable_parent(target, identity, &parent);
    if (status)
        goto out;

    status = parent->ops->dir.mkdir ? parent->ops->dir.mkdir(parent, &target, mode, identity) : EPERM;
    vfs_node_unref(parent);

out:
    vfs_path_drop(target);
    return status;
}

errno_t vfs_symlink(
    struct vfs_path root,
    struct vfs_path cwd,
    const char* path,
    const char* target_path,
    const struct ident* identity
) {
    struct vfs_path target;
    errno_t status = vfs_path_lookup(root, cwd, path, identity, VFS_LOOKUP_MUST_NOT_EXIST, &target);
    if (status)
        return status;

    struct vfs_node* parent;
    status = get_writable_parent(target, identity, &parent);
    if (status)
        goto out;

    status = parent->ops->dir.symlink
        ? parent->ops->dir.symlink(parent, &target, target_path, strlen(target_path), identity)
        : EPERM;
    vfs_node_unref(parent);

out:
    vfs_path_drop(target);
    return status;
}

static bool last_component_is(const char* path, const char* name) {
    size_t len = strlen(path);
    while (len > 1 && path[len - 1] == '/')
        len--;
    size_t begin = len;
    while (begin > 0 && path[begin - 1] != '/')
        begin--;
    return len - begin == strlen(name) && !memcmp(path + begin, name, len - begin);
}

static errno_t check_sticky(struct vfs_node* dir, struct vfs_node* victim, const struct ident* identity) {
    if (identity->euid == 0)
        return 0;

    mutex_lock(&dir->lock);
    const bool sticky = dir->mode & __S_ISVTX;
    const uid_t dir_owner = dir->uid;
    mutex_unlock(&dir->lock);
    if (!sticky)
        return 0;

    mutex_lock(&victim->lock);
    const uid_t owner = victim->uid;
    mutex_unlock(&victim->lock);
    return identity->euid == owner || identity->euid == dir_owner ? 0 : EPERM;
}

errno_t vfs_unlink(struct vfs_path root, struct vfs_path cwd, const char* path, const struct ident* identity) {
    struct vfs_path target;
    errno_t status = vfs_path_lookup(root, cwd, path, identity, VFS_LOOKUP_MUST_EXIST, &target);
    if (status)
        return status;

    struct vfs_node* node = vfs_entry_get_inode(target.entry);
    if (!node) {
        status = ENOENT;
        goto out;
    }
    if (node->type == VNODE_DIRECTORY) {
        status = EISDIR;
        goto out_node;
    }

    struct vfs_node* parent;
    status = get_writable_parent(target, identity, &parent);
    if (status)
        goto out_node;

    status = check_sticky(parent, node, identity);
    if (!status)
        status = parent->ops->dir.unlink ? parent->ops->dir.unlink(parent, &target, identity) : EPERM;
    vfs_node_unref(parent);

out_node:
    vfs_node_unref(node);
out:
    vfs_path_drop(target);
    return status;
}

// Probe for a single entry to check if the directory is empty.
static errno_t check_dir_empty(struct vfs_node* node, struct vfs_entry* entry, const struct ident* identity) {
    struct __dirent probe;
    size_t count = 0;
    if (node->ops->dir.get_dir_entries) {
        errno_t status = node->ops->dir.get_dir_entries(node, entry, 0, &probe, 1, identity, &count);
        if (status)
            return status;
    }
    return count > 0 ? ENOTEMPTY : 0;
}

errno_t vfs_rmdir(struct vfs_path root, struct vfs_path cwd, const char* path, const struct ident* identity) {
    if (last_component_is(path, "."))
        return EINVAL;
    if (last_component_is(path, ".."))
        return ENOTEMPTY;

    struct vfs_path target;
    errno_t status = vfs_path_lookup(root, cwd, path, identity, VFS_LOOKUP_MUST_EXIST, &target);
    if (status)
        return status;

    struct vfs_node* node = vfs_entry_get_inode(target.entry);
    if (!node) {
        status = ENOENT;
        goto out;
    }
    if (node->type != VNODE_DIRECTORY) {
        status = ENOTDIR;
        goto out_node;
    }
    if (target.entry == target.mount->root) {
        status = EBUSY;
        goto out_node;
    }

    status = check_dir_empty(node, target.entry, identity);
    if (status)
        goto out_node;

    struct vfs_node* parent;
    status = get_writable_parent(target, identity, &parent);
    if (status)
        goto out_node;

    status = check_sticky(parent, node, identity);
    if (!status) {
        if (parent->ops->dir.rmdir)
            status = parent->ops->dir.rmdir(parent, &target, identity);
        else if (parent->ops->dir.unlink)
            status = parent->ops->dir.unlink(parent, &target, identity);
        else
            status = EPERM;
    }
    vfs_node_unref(parent);

out_node:
    vfs_node_unref(node);
out:
    vfs_path_drop(target);
    return status;
}

errno_t vfs_mknod(
    struct vfs_path root,
    struct vfs_path cwd,
    const char* path,
    mode_t mode,
    struct vfs_mknod_target* target,
    const struct ident* identity,
    struct vfs_node** out
) {
    struct vfs_path new_path;
    errno_t status = vfs_path_lookup(root, cwd, path, identity, VFS_LOOKUP_MUST_NOT_EXIST, &new_path);
    if (status)
        return status;

    struct vfs_node* parent;
    status = get_writable_parent(new_path, identity, &parent);
    if (status)
        goto out;

    status = parent->ops->dir.mknod ? parent->ops->dir.mknod(parent, &new_path, mode, target, identity) : ENODEV;
    vfs_node_unref(parent);
    if (!status && out) {
        *out = vfs_entry_get_inode(new_path.entry);
        if (!*out)
            status = ENOENT;
    }

out:
    vfs_path_drop(new_path);
    return status;
}

static bool entry_is_ancestor(struct vfs_entry* ancestor, struct vfs_entry* entry) {
    for (struct vfs_entry* it = entry; it; it = it->parent) {
        if (it == ancestor)
            return true;
    }
    return false;
}

static bool entry_points_to(struct vfs_entry* entry, struct vfs_node* node) {
    struct vfs_node* current = vfs_entry_get_inode(entry);
    if (current)
        vfs_node_unref(current);
    return current == node;
}

static errno_t check_rename_target(struct vfs_node* moved, struct vfs_path target, const struct ident* identity) {
    struct vfs_node* existing = vfs_entry_get_inode(target.entry);
    if (!existing)
        return 0;

    errno_t status = 0;
    if (moved->type == VNODE_DIRECTORY && existing->type != VNODE_DIRECTORY)
        status = ENOTDIR;
    else if (moved->type != VNODE_DIRECTORY && existing->type == VNODE_DIRECTORY)
        status = EISDIR;
    else if (existing->type == VNODE_DIRECTORY)
        status = check_dir_empty(existing, target.entry, identity);

    vfs_node_unref(existing);
    return status;
}

static errno_t check_rename_perms(
    struct vfs_node* moved,
    struct vfs_path new,
    struct vfs_node* old_parent,
    struct vfs_node* new_parent,
    const struct ident* identity
) {
    errno_t status = check_sticky(old_parent, moved, identity);
    if (status)
        return status;

    struct vfs_node* existing = vfs_entry_get_inode(new.entry);
    if (existing) {
        status = check_sticky(new_parent, existing, identity);
        vfs_node_unref(existing);
        if (status)
            return status;
    }

    if (moved->type == VNODE_DIRECTORY && old_parent != new_parent)
        return vfs_node_try_access(moved, identity, W_OK, false);
    return 0;
}

static errno_t do_rename(
    struct vfs_path old,
    struct vfs_path new,
    struct vfs_node* moved,
    const struct ident* identity
) {
    struct vfs_node* old_parent;
    errno_t status = get_writable_parent(old, identity, &old_parent);
    if (status)
        return status;

    struct vfs_node* new_parent;
    status = get_writable_parent(new, identity, &new_parent);
    if (status) {
        vfs_node_unref(old_parent);
        return status;
    }

    status = check_rename_perms(moved, new, old_parent, new_parent, identity);
    if (!status) {
        if (old_parent->ops->dir.rename)
            status = old_parent->ops->dir.rename(old_parent, &old, new_parent, &new, identity);
        else
            status = EPERM;
    }
    vfs_node_unref(new_parent);
    vfs_node_unref(old_parent);
    return status;
}

errno_t vfs_rename(
    struct vfs_path root,
    struct vfs_path old_cwd,
    const char* old_path,
    struct vfs_path new_cwd,
    const char* new_path,
    const struct ident* identity
) {
    if (last_component_is(old_path, ".") || last_component_is(old_path, "..") || last_component_is(new_path, ".") ||
        last_component_is(new_path, ".."))
        return EINVAL;

    struct vfs_path old;
    errno_t status = vfs_path_lookup(root, old_cwd, old_path, identity, VFS_LOOKUP_MUST_EXIST, &old);
    if (status)
        return status;

    struct vfs_path new;
    status = vfs_path_lookup(root, new_cwd, new_path, identity, 0, &new);
    if (status)
        goto out_old;

    if (old.mount != new.mount) {
        status = EXDEV;
        goto out_new;
    }
    if (old.entry == old.mount->root || new.entry == new.mount->root) {
        status = EBUSY;
        goto out_new;
    }
    if (old.entry == new.entry)
        goto out_new;

    struct vfs_node* moved = vfs_entry_get_inode(old.entry);
    if (!moved) {
        status = ENOENT;
        goto out_new;
    }
    if (moved->type == VNODE_DIRECTORY && entry_is_ancestor(old.entry, new.entry))
        status = EINVAL;
    if (!status && !entry_points_to(new.entry, moved)) {
        status = check_rename_target(moved, new, identity);
        if (!status)
            status = do_rename(old, new, moved, identity);
    }

    vfs_node_unref(moved);
out_new:
    vfs_path_drop(new);
out_old:
    vfs_path_drop(old);
    return status;
}

errno_t vfs_link(
    struct vfs_path root,
    struct vfs_path old_cwd,
    const char* old_path,
    struct vfs_path new_cwd,
    const char* new_path,
    bool follow_symlinks,
    const struct ident* identity
) {
    enum vfs_lookup_flags flags = VFS_LOOKUP_MUST_EXIST;
    if (follow_symlinks)
        flags |= VFS_LOOKUP_FOLLOW_SYMLINKS;

    struct vfs_path old;
    errno_t status = vfs_path_lookup(root, old_cwd, old_path, identity, flags, &old);
    if (status)
        return status;

    struct vfs_node* target = vfs_entry_get_inode(old.entry);
    if (!target) {
        status = ENOENT;
        goto out_old;
    }
    if (target->type == VNODE_DIRECTORY) {
        status = EPERM;
        goto out_target;
    }

    struct vfs_path new;
    status = vfs_path_lookup(root, new_cwd, new_path, identity, VFS_LOOKUP_MUST_NOT_EXIST, &new);
    if (status)
        goto out_target;

    if (new.mount != old.mount) {
        status = EXDEV;
        goto out_new;
    }

    struct vfs_node* parent;
    status = get_writable_parent(new, identity, &parent);
    if (status)
        goto out_new;

    status = parent->ops->dir.link ? parent->ops->dir.link(parent, &new, target, identity) : EPERM;
    vfs_node_unref(parent);

out_new:
    vfs_path_drop(new);
out_target:
    vfs_node_unref(target);
out_old:
    vfs_path_drop(old);
    return status;
}

static mode_t node_type_mode(enum vfs_node_type type) {
    switch (type) {
    case VNODE_DIRECTORY:
        return __S_IFDIR;
    case VNODE_SYMLINK:
        return __S_IFLNK;
    case VNODE_FIFO:
        return __S_IFIFO;
    case VNODE_BLOCKDEV:
        return __S_IFBLK;
    case VNODE_CHARDEV:
        return __S_IFCHR;
    case VNODE_SOCKET:
        return __S_IFSOCK;
    default:
        return __S_IFREG;
    }
}

void vfs_stat(struct vfs_node* node, struct __stat* out) {
    mutex_lock(&node->lock);
    *out = (struct __stat){
        .st_ino = node->id,
        .st_mode = node->mode | node_type_mode(node->type),
        .st_nlink = node->nlink,
        .st_uid = node->uid,
        .st_gid = node->gid,
        .st_rdev = node->rdev,
        .st_size = node->size,
        .st_atim = node->atime,
        .st_mtim = node->mtime,
        .st_ctim = node->ctime,
        .st_blksize = 4096, // TODO: Get from file system
        .st_blocks = node->blocks,
    };
    mutex_unlock(&node->lock);
}

static bool is_owner(struct vfs_node* node, const struct ident* identity) {
    return identity->euid == 0 || identity->euid == node->uid;
}

errno_t vfs_chmod(struct vfs_node* node, mode_t mode, const struct ident* identity) {
    if (is_read_only(node))
        return EROFS;

    mutex_lock(&node->lock);
    if (!is_owner(node, identity)) {
        mutex_unlock(&node->lock);
        return EPERM;
    }
    mode &= 07777;
    if (identity->euid != 0 && node->type == VNODE_REGULAR && !ident_in_group(identity, node->gid, false))
        mode &= ~__S_ISGID;
    node->mode = mode;
    node->ctime = now();
    mutex_unlock(&node->lock);

    return sync_node(node);
}

errno_t vfs_chown(struct vfs_node* node, uid_t uid, gid_t gid, const struct ident* identity) {
    if (is_read_only(node))
        return EROFS;

    mutex_lock(&node->lock);
    if (identity->euid != 0) {
        const bool uid_ok = uid == (uid_t)-1 || uid == node->uid;
        const bool gid_ok = gid == (gid_t)-1 || gid == node->gid || ident_in_group(identity, gid, false);
        if (identity->euid != node->uid || !uid_ok || !gid_ok) {
            mutex_unlock(&node->lock);
            return EPERM;
        }
    }

    if (uid == (uid_t)-1 && gid == (gid_t)-1) {
        mutex_unlock(&node->lock);
        return 0;
    }

    if (uid != (uid_t)-1)
        node->uid = uid;
    if (gid != (gid_t)-1)
        node->gid = gid;
    if (identity->euid != 0 && node->type == VNODE_REGULAR && (node->mode & (__S_IXUSR | __S_IXGRP | __S_IXOTH)))
        node->mode &= ~(__S_ISUID | __S_ISGID);
    node->ctime = now();
    mutex_unlock(&node->lock);

    return sync_node(node);
}

static bool valid_utime(const timespec_t* ts) {
    return ts->tv_nsec == __UTIME_NOW || ts->tv_nsec == __UTIME_OMIT ||
        (ts->tv_nsec >= 0 && ts->tv_nsec < 1'000'000'000);
}

errno_t vfs_utimens(struct vfs_node* node, const timespec_t* times, const struct ident* identity) {
    const timespec_t current = now();
    timespec_t ts[2] = {
        {.tv_nsec = __UTIME_NOW},
        {.tv_nsec = __UTIME_NOW},
    };
    if (times) {
        ts[0] = times[0];
        ts[1] = times[1];
        if (!valid_utime(&ts[0]) || !valid_utime(&ts[1]))
            return EINVAL;
    }

    if (ts[0].tv_nsec == __UTIME_OMIT && ts[1].tv_nsec == __UTIME_OMIT)
        return 0;
    if (is_read_only(node))
        return EROFS;

    const bool only_now = (ts[0].tv_nsec == __UTIME_NOW || ts[0].tv_nsec == __UTIME_OMIT) &&
        (ts[1].tv_nsec == __UTIME_NOW || ts[1].tv_nsec == __UTIME_OMIT);

    mutex_lock(&node->lock);
    const bool owner = is_owner(node, identity);
    mutex_unlock(&node->lock);

    if (!owner) {
        if (!only_now)
            return EPERM;
        errno_t status = vfs_node_try_access(node, identity, W_OK, false);
        if (status)
            return status;
    }

    mutex_lock(&node->lock);
    for (size_t i = 0; i < 2; i++) {
        if (ts[i].tv_nsec == __UTIME_OMIT)
            continue;
        timespec_t* target = i == 0 ? &node->atime : &node->mtime;
        *target = ts[i].tv_nsec == __UTIME_NOW ? current : ts[i];
    }
    node->ctime = current;
    mutex_unlock(&node->lock);

    return sync_node(node);
}

errno_t vfs_node_check_exec(struct vfs_node* node, const struct ident* identity) {
    if (node->type != VNODE_REGULAR)
        return EACCES;
    return vfs_node_try_access(node, identity, X_OK, false);
}

errno_t vfs_truncate(struct vfs_node* node, uint64_t length) {
    if (node->type == VNODE_DIRECTORY)
        return EISDIR;
    if (node->type != VNODE_REGULAR)
        return EINVAL;
    if (!node->ops->regular.truncate)
        return EPERM;
    return node->ops->regular.truncate(node, length);
}

errno_t vfs_read_link(struct vfs_node* node, iovec_iter_t* iter, size_t* out_read) {
    if (node->type != VNODE_SYMLINK)
        return EINVAL;
    if (!node->ops->symlink.read_link)
        return EINVAL;
    return node->ops->symlink.read_link(node, iter, out_read);
}

errno_t vfs_statvfs(struct vfs_node* node, struct __statvfs* out) {
    if (!node->sb || !node->sb->ops || !node->sb->ops->statvfs)
        return ENOSYS;
    return node->sb->ops->statvfs(node->sb, out);
}
