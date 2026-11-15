// SPDX-License-Identifier: BSD-3-Clause

#include <fs/private.h>
#include <fs/vfs.h>
#include <lib/assert.h>
#include <lib/clock.h>
#include <mem/alloc.h>
#include <string.h>

errno_t vfs_node_new(
    struct vfs_super_block* sb,
    enum vfs_node_type type,
    const struct vfs_node_ops* ops,
    mode_t mode,
    struct vfs_node** out
) {
    struct vfs_node* node = mem_alloc(sizeof(*node), 0);
    if (!node)
        return ENOMEM;

    refcount_init(&node->rc);
    node->type = type;
    node->ops = ops;
    node->file_ops = ops ? ops->file_ops : nullptr;
    node->sb = sb;
    if (sb)
        vfs_super_block_ref(sb);
    mutex_init(&node->lock);
    node->mode = mode & 07777;
    node->nlink = 1;
    const timespec_t now = time_from_nano(clock_get_realtime_ns());
    node->atime = now;
    node->mtime = now;
    node->ctime = now;

    *out = node;
    return 0;
}

void vfs_node_ref(struct vfs_node* node) {
    refcount_inc(&node->rc);
}

void vfs_node_unref(struct vfs_node* node) {
    if (!refcount_dec(&node->rc))
        return;

    if (node->ops && node->ops->destroy)
        node->ops->destroy(node);
    if (node->type == VNODE_FIFO && node->pipe)
        pipe_destroy(node->pipe);
    if (node->sb)
        vfs_super_block_unref(node->sb);
    mem_free(node, sizeof(*node));
}

errno_t vfs_entry_new(const char* name, size_t name_len, struct vfs_entry* parent, struct vfs_entry** out) {
    struct vfs_entry* entry = mem_alloc(sizeof(*entry), 0);
    if (!entry)
        return ENOMEM;

    entry->name = mem_alloc(name_len + 1, 0);
    if (!entry->name) {
        mem_free(entry, sizeof(*entry));
        return ENOMEM;
    }
    memcpy(entry->name, name, name_len);
    entry->name[name_len] = '\0';
    entry->name_len = name_len;

    refcount_init(&entry->rc);
    mutex_init(&entry->lock);
    entry->state = VFS_ENTRY_NOT_CACHED;
    entry->inode = nullptr;
    entry->parent = parent;
    if (parent)
        vfs_entry_ref(parent);
    tailq_init(&entry->children);
    vec_init(&entry->mounts);

    *out = entry;
    return 0;
}

void vfs_entry_ref(struct vfs_entry* entry) {
    refcount_inc(&entry->rc);
}

void vfs_entry_unref(struct vfs_entry* entry) {
    if (!refcount_dec(&entry->rc))
        return;

    // Children and mounts hold references to this entry, so both must be gone by now.
    assert(tailq_empty(&entry->children), "Destroying an entry that still has children");
    assert(vec_is_empty(&entry->mounts), "Destroying an entry that is still a mount point");

    if (entry->inode)
        vfs_node_unref(entry->inode);
    if (entry->parent)
        vfs_entry_unref(entry->parent);
    vec_destroy(&entry->mounts);
    mem_free(entry->name, entry->name_len + 1);
    mem_free(entry, sizeof(*entry));
}

void vfs_entry_set_inode(struct vfs_entry* entry, struct vfs_node* node) {
    vfs_node_ref(node);

    mutex_lock(&entry->lock);
    struct vfs_node* old = entry->inode;
    entry->inode = node;
    entry->state = VFS_ENTRY_PRESENT;
    mutex_unlock(&entry->lock);

    if (old)
        vfs_node_unref(old);
}

void vfs_entry_set_negative(struct vfs_entry* entry) {
    mutex_lock(&entry->lock);
    struct vfs_node* old = entry->inode;
    entry->inode = nullptr;
    entry->state = VFS_ENTRY_NOT_PRESENT;
    mutex_unlock(&entry->lock);

    if (old)
        vfs_node_unref(old);
}

void vfs_entry_replace_children(struct vfs_entry* dst, struct vfs_entry* src) {
    tailq_head(struct vfs_entry) stale;
    tailq_init(&stale);

    // Avoid a potential deadlock when this function is called with (a, b) and (b, a) at the same time.
    struct vfs_entry* first = src < dst ? src : dst;
    struct vfs_entry* second = src < dst ? dst : src;
    mutex_lock(&first->lock);
    mutex_lock(&second->lock);
    struct vfs_entry* child;
    while ((child = tailq_first(&dst->children))) {
        tailq_remove(&dst->children, child, child_link);
        tailq_insert_tail(&stale, child, child_link);
    }
    while ((child = tailq_first(&src->children))) {
        tailq_remove(&src->children, child, child_link);
        child->parent = dst;
        vfs_entry_ref(dst);
        vfs_entry_unref(src);
        tailq_insert_tail(&dst->children, child, child_link);
    }
    mutex_unlock(&second->lock);
    mutex_unlock(&first->lock);

    while ((child = tailq_first(&stale))) {
        tailq_remove(&stale, child, child_link);
        vfs_entry_unref(child);
    }
}

void vfs_entry_prune(struct vfs_entry* entry) {
    tailq_head(struct vfs_entry) stale;
    tailq_init(&stale);

    mutex_lock(&entry->lock);
    struct vfs_entry* child = tailq_first(&entry->children);
    while (child) {
        struct vfs_entry* next = tailq_next(child, child_link);
        vfs_entry_prune(child);

        mutex_lock(&child->lock);
        const bool idle = tailq_empty(&child->children) && vec_is_empty(&child->mounts);
        mutex_unlock(&child->lock);
        if (idle && atomic_load(&child->rc.count, ATOMIC_RELAXED) == 1) {
            tailq_remove(&entry->children, child, child_link);
            tailq_insert_tail(&stale, child, child_link);
        }
        child = next;
    }
    mutex_unlock(&entry->lock);

    while ((child = tailq_first(&stale))) {
        tailq_remove(&stale, child, child_link);
        vfs_entry_unref(child);
    }
}

struct vfs_node* vfs_entry_get_inode(struct vfs_entry* entry) {
    struct vfs_node* node = nullptr;

    mutex_lock(&entry->lock);
    if (entry->state == VFS_ENTRY_PRESENT) {
        node = entry->inode;
        vfs_node_ref(node);
    }
    mutex_unlock(&entry->lock);

    return node;
}

void vfs_mount_ref(struct vfs_mount* mount) {
    refcount_inc(&mount->rc);
}

void vfs_mount_unref(struct vfs_mount* mount) {
    if (!refcount_dec(&mount->rc))
        return;

    if (mount->has_mount_point)
        vfs_path_drop(mount->mount_point);
    vfs_entry_unref(mount->root);
    mem_free(mount, sizeof(*mount));
}

void vfs_super_block_ref(struct vfs_super_block* sb) {
    refcount_inc(&sb->rc);
}

void vfs_super_block_unref(struct vfs_super_block* sb) {
    if (!refcount_dec(&sb->rc))
        return;

    if (sb->ops && sb->ops->destroy)
        sb->ops->destroy(sb);
}

void vfs_file_ref(struct vfs_file* file) {
    refcount_inc(&file->rc);
}

static void file_free(struct epoch_head* head) {
    mem_free(containerof(head, struct vfs_file, epoch), sizeof(struct vfs_file));
}

void vfs_file_unref(struct vfs_file* file) {
    if (!refcount_dec(&file->rc))
        return;

    if (file->ops && file->ops->release)
        file->ops->release(file);
    if (file->has_path)
        vfs_path_drop(file->path);
    vfs_node_unref(file->vnode);
    if (file->absolute_path)
        mem_free(file->absolute_path, strlen(file->absolute_path) + 1);
    epoch_call(&file->epoch, file_free);
}

struct vfs_path vfs_path_clone(struct vfs_path path) {
    vfs_mount_ref(path.mount);
    vfs_entry_ref(path.entry);
    return path;
}

void vfs_path_drop(struct vfs_path path) {
    vfs_entry_unref(path.entry);
    vfs_mount_unref(path.mount);
}
