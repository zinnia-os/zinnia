// SPDX-License-Identifier: BSD-3-Clause

#include <fs/private.h>
#include <fs/vfs.h>
#include <mem/alloc.h>
#include <string.h>

constexpr size_t MAX_SYMLINK_DEPTH = 40;

// If `path` is a mount point, crosses into the root of the topmost mounted file system.
// Returns a new reference.
static struct vfs_path cross_mounts(struct vfs_path path) {
    struct vfs_mount* mount = path.mount;
    struct vfs_entry* entry = path.entry;

again:
    mutex_lock(&entry->lock);
    vec_foreach(&entry->mounts, i) {
        struct vfs_mount* child = vec_elem_get(&entry->mounts, i);
        if (child != mount) {
            mutex_unlock(&entry->lock);
            mount = child;
            entry = child->root;
            goto again;
        }
    }
    mutex_unlock(&entry->lock);

    return vfs_path_clone((struct vfs_path){.mount = mount, .entry = entry});
}

errno_t vfs_path_lookup_parent(struct vfs_path path, struct vfs_path* out) {
    struct vfs_mount* mount = path.mount;
    struct vfs_entry* entry = path.entry;

    // Get the top mount point.
    while (mount->has_mount_point && entry == mount->root) {
        entry = mount->mount_point.entry;
        mount = mount->mount_point.mount;
    }

    if (!entry->parent)
        return ENOENT;

    *out = vfs_path_clone((struct vfs_path){.mount = mount, .entry = entry->parent});
    return 0;
}

errno_t vfs_path_lookup_child(
    struct vfs_path path,
    struct vfs_path* root,
    const char* name,
    size_t name_len,
    struct vfs_path* out
) {
    errno_t status = 0;

    // Traverse the mounts on the current entry.
    struct vfs_path crossed = cross_mounts(path);

    // A lookup only makes sense on directories.
    struct vfs_node* parent = vfs_entry_get_inode(crossed.entry);
    if (!parent) {
        status = ENOENT;
        goto fail;
    }
    if (parent->type != VNODE_DIRECTORY) {
        status = ENOTDIR;
        goto fail;
    }

    if (name_len == 1 && name[0] == '.') {
        vfs_node_unref(parent);
        *out = crossed;
        return 0;
    }

    if (name_len == 2 && name[0] == '.' && name[1] == '.') {
        vfs_node_unref(parent);

        struct vfs_path crossed_root = cross_mounts(*root);
        bool at_root = vfs_path_equal(crossed, crossed_root);
        vfs_path_drop(crossed_root);
        if (at_root) {
            *out = crossed;
            return 0;
        }

        status = vfs_path_lookup_parent(crossed, out);
        if (status == ENOENT) {
            *out = crossed;
            return 0;
        }
        vfs_path_drop(crossed);
        return status;
    }

    mutex_lock(&crossed.entry->lock);

    // If this entry has already been looked up before, return that.
    struct vfs_entry* child = nullptr;
    for (struct vfs_entry* it = tailq_first(&crossed.entry->children); it; it = tailq_next(it, child_link)) {
        if (it->name_len == name_len && !memcmp(it->name, name, name_len)) {
            child = it;
            vfs_entry_ref(child);
            break;
        }
    }

    if (!child) {
        status = vfs_entry_new(name, name_len, crossed.entry, &child);
        if (status) {
            mutex_unlock(&crossed.entry->lock);
            goto fail;
        }

        struct vfs_path child_path = {.mount = crossed.mount, .entry = child};
        status = parent->ops->dir.lookup ? parent->ops->dir.lookup(parent, &child_path) : ENOENT;
        if (status && status != ENOENT) {
            mutex_unlock(&crossed.entry->lock);
            vfs_entry_unref(child);
            goto fail;
        }

        // Allow lookup failures so we can cache it as a negative entry.
        if (status == ENOENT)
            vfs_entry_set_negative(child);
        status = 0;

        // The children list takes over the initial reference.
        tailq_insert_tail(&crossed.entry->children, child, child_link);
        vfs_entry_ref(child);
    }

    mutex_unlock(&crossed.entry->lock);

    *out = cross_mounts((struct vfs_path){.mount = crossed.mount, .entry = child});
    vfs_entry_unref(child);
    vfs_node_unref(parent);
    vfs_path_drop(crossed);
    return 0;

fail:
    if (parent)
        vfs_node_unref(parent);
    vfs_path_drop(crossed);
    return status;
}

static errno_t lookup_inner(
    struct vfs_path root,
    struct vfs_path start,
    const char* path,
    const struct ident* identity,
    enum vfs_lookup_flags flags,
    size_t* symlink_depth,
    struct vfs_path* out
);

// Resolves `current` in place until it no longer points to a symbolic link.
static errno_t resolve_symlink(
    struct vfs_path root,
    struct vfs_path* current,
    const struct ident* identity,
    enum vfs_lookup_flags flags,
    size_t* symlink_depth
) {
    errno_t status = 0;
    char* link_buf = nullptr;

    while (true) {
        struct vfs_node* node = vfs_entry_get_inode(current->entry);
        if (!node)
            break;
        if (node->type != VNODE_SYMLINK) {
            vfs_node_unref(node);
            break;
        }

        if (*symlink_depth >= MAX_SYMLINK_DEPTH || !node->ops->symlink.read_link) {
            vfs_node_unref(node);
            status = ELOOP;
            break;
        }
        (*symlink_depth)++;

        if (!link_buf) {
            link_buf = mem_alloc(__PATH_MAX + 1, ALLOC_NOZERO);
            if (!link_buf) {
                vfs_node_unref(node);
                status = ENOMEM;
                break;
            }
        }

        kvec_t vec = {.iov_base = link_buf, .iov_len = __PATH_MAX};
        iovec_iter_t iter = iovec_iter_kernel(&vec, 1);
        size_t link_len = 0;
        status = node->ops->symlink.read_link(node, &iter, &link_len);
        vfs_node_unref(node);
        if (status)
            break;
        link_buf[link_len] = '\0';

        struct vfs_path parent;
        status = vfs_path_lookup_parent(*current, &parent);
        if (status)
            break;

        struct vfs_path result;
        status = lookup_inner(
            root,
            parent,
            link_buf,
            identity,
            flags & ~(VFS_LOOKUP_MUST_EXIST | VFS_LOOKUP_MUST_NOT_EXIST),
            symlink_depth,
            &result
        );
        vfs_path_drop(parent);
        if (status)
            break;

        if (vfs_path_equal(result, *current)) {
            vfs_path_drop(result);
            status = ELOOP;
            break;
        }

        vfs_path_drop(*current);
        *current = result;
    }

    mem_free(link_buf, __PATH_MAX + 1);
    return status;
}

static errno_t lookup_inner(
    struct vfs_path root,
    struct vfs_path start,
    const char* path,
    const struct ident* identity,
    enum vfs_lookup_flags flags,
    size_t* symlink_depth,
    struct vfs_path* out
) {
    errno_t status = 0;

    if (path[0] == '\0')
        return ENOENT;

    // If a path starts with '/', it's an absolute path.
    // In that case, skip the first character and use the current root as a starting point.
    struct vfs_path current;
    if (path[0] == '/') {
        current = vfs_path_clone(root);
        path++;
    } else {
        current = vfs_path_clone(start);
    }

    // Parse each component.
    while (true) {
        while (*path == '/')
            path++;
        if (*path == '\0')
            break;

        const char* component = path;
        size_t component_len = 0;
        while (path[component_len] != '/' && path[component_len] != '\0')
            component_len++;
        path += component_len;

        if (component_len > __NAME_MAX) {
            status = ENAMETOOLONG;
            goto fail;
        }

        status = resolve_symlink(root, &current, identity, flags, symlink_depth);
        if (status)
            goto fail;

        struct vfs_node* node = vfs_entry_get_inode(current.entry);
        if (!node) {
            status = ENOENT;
            goto fail;
        }
        if (node->type != VNODE_DIRECTORY) {
            vfs_node_unref(node);
            status = ENOTDIR;
            goto fail;
        }

        status = vfs_node_try_access(node, identity, X_OK, flags & VFS_LOOKUP_REAL_ID);
        vfs_node_unref(node);
        if (status)
            goto fail;

        struct vfs_path next;
        status = vfs_path_lookup_child(current, &root, component, component_len, &next);
        if (status)
            goto fail;
        vfs_path_drop(current);
        current = next;
    }

    if (flags & VFS_LOOKUP_FOLLOW_SYMLINKS) {
        status = resolve_symlink(root, &current, identity, flags, symlink_depth);
        if (status)
            goto fail;
    }

    struct vfs_node* node = vfs_entry_get_inode(current.entry);
    if (node)
        vfs_node_unref(node);

    if ((flags & VFS_LOOKUP_MUST_EXIST) && !node) {
        status = ENOENT;
        goto fail;
    }
    if ((flags & VFS_LOOKUP_MUST_NOT_EXIST) && node) {
        status = EEXIST;
        goto fail;
    }

    *out = current;
    return 0;

fail:
    vfs_path_drop(current);
    return status;
}

errno_t vfs_path_lookup(
    struct vfs_path root,
    struct vfs_path cwd,
    const char* path,
    const struct ident* identity,
    enum vfs_lookup_flags flags,
    struct vfs_path* out
) {
    size_t symlink_depth = 0;
    return lookup_inner(root, cwd, path, identity, flags, &symlink_depth, out);
}

errno_t vfs_path_absolute(struct vfs_path path, struct vfs_path* root, char* buf, size_t len, size_t* out_len) {
    if (len == 0)
        return ENAMETOOLONG;

    struct vfs_path global_root;
    if (!root) {
        global_root = vfs_get_root();
        root = &global_root;
    }

    errno_t status = 0;
    struct vfs_mount* mount = path.mount;
    struct vfs_entry* entry = path.entry;

    // Build the path back to front, leaving room for the NUL terminator.
    size_t cursor = len - 1;
    buf[cursor] = '\0';

    while (entry != root->entry || mount != root->mount) {
        // At the root of a mounted file system we must step into the parent file system.
        if (entry == mount->root) {
            if (!mount->has_mount_point) {
                // Global root with no parent.
                status = ENOENT;
                goto out;
            }
            entry = mount->mount_point.entry;
            mount = mount->mount_point.mount;
            continue;
        }

        if (entry->name_len + 1 > cursor) {
            status = ENAMETOOLONG;
            goto out;
        }
        cursor -= entry->name_len;
        memcpy(buf + cursor, entry->name, entry->name_len);
        buf[--cursor] = '/';

        if (!entry->parent) {
            status = ENOENT;
            goto out;
        }
        entry = entry->parent;
    }

    // The root itself resolves to "/".
    if (cursor == len - 1) {
        if (cursor == 0) {
            status = ENAMETOOLONG;
            goto out;
        }
        buf[--cursor] = '/';
    }

    size_t result_len = len - 1 - cursor;
    memmove(buf, buf + cursor, result_len + 1);
    if (out_len)
        *out_len = result_len;

out:
    if (root == &global_root)
        vfs_path_drop(global_root);
    return status;
}
