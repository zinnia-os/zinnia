// SPDX-License-Identifier: BSD-3-Clause

#include <fs/private.h>
#include <fs/vfs.h>
#include <lib/assert.h>
#include <lib/mutex.h>
#include <lib/print.h>
#include <mem/alloc.h>
#include <string.h>

// All known and registered file systems.
static mutex_t fs_table_lock;
static vec(const struct vfs_filesystem*) fs_table;

// Super blocks of all mounted file systems. Each holds a reference.
static mutex_t supers_lock;
static vec(struct vfs_super_block*) mounted_supers;

// The global root.
static struct vfs_path vfs_root;
static bool vfs_root_set;

void vfs_set_root(struct vfs_path root) {
    assert(!vfs_root_set, "The VFS root can only be set once");
    vfs_root = vfs_path_clone(root);
    vfs_root_set = true;
}

struct vfs_path vfs_get_root() {
    assert(vfs_root_set, "The VFS root hasn't been set yet");
    return vfs_path_clone(vfs_root);
}

errno_t vfs_register_filesystem(const struct vfs_filesystem* fs) {
    errno_t status = 0;

    mutex_lock(&fs_table_lock);
    vec_foreach(&fs_table, i) {
        if (!strcmp(vec_elem_get(&fs_table, i)->name, fs->name)) {
            status = EEXIST;
            goto out;
        }
    }
    status = vec_push(&fs_table, fs);

out:
    mutex_unlock(&fs_table_lock);
    if (!status)
        log("vfs: Registered new file system \"%s\"\n", fs->name);
    return status;
}

// Returns a new reference to the super block of `mount`, if it has one.
static struct vfs_super_block* mount_get_super(struct vfs_mount* mount) {
    struct vfs_node* root = vfs_entry_get_inode(mount->root);
    if (!root)
        return nullptr;

    struct vfs_super_block* sb = root->sb;
    if (sb)
        vfs_super_block_ref(sb);
    vfs_node_unref(root);
    return sb;
}

errno_t vfs_mount(const char* fs_name, enum mount_flags flags, void* arg, struct vfs_mount** out) {
    const struct vfs_filesystem* fs = nullptr;

    mutex_lock(&fs_table_lock);
    vec_foreach(&fs_table, i) {
        if (!strcmp(vec_elem_get(&fs_table, i)->name, fs_name)) {
            fs = vec_elem_get(&fs_table, i);
            break;
        }
    }
    mutex_unlock(&fs_table_lock);

    if (!fs) {
        log_warn("Attempted to mount unknown file system type \"%s\"\n", fs_name);
        return ENODEV;
    }

    struct vfs_mount* mount;
    errno_t status = fs->mount(fs, flags, arg, &mount);
    if (status)
        return status;

    struct vfs_super_block* sb = mount_get_super(mount);
    if (sb) {
        bool known = false;
        mutex_lock(&supers_lock);
        vec_foreach(&mounted_supers, i) {
            if (vec_elem_get(&mounted_supers, i) == sb) {
                known = true;
                break;
            }
        }
        // The list takes over our reference.
        if (!known && vec_push(&mounted_supers, sb) == 0)
            sb = nullptr;
        mutex_unlock(&supers_lock);

        if (sb)
            vfs_super_block_unref(sb);
    }

    *out = mount;
    return 0;
}

errno_t vfs_unmount(struct vfs_mount* mount) {
    struct vfs_super_block* sb = mount_get_super(mount);
    if (!sb)
        return 0;

    mutex_lock(&supers_lock);
    vec_foreach(&mounted_supers, i) {
        if (vec_elem_get(&mounted_supers, i) == sb) {
            vec_swap_remove(&mounted_supers, i);
            vfs_super_block_unref(sb);
            break;
        }
    }
    mutex_unlock(&supers_lock);

    errno_t status = sb->ops && sb->ops->sync ? sb->ops->sync(sb) : 0;
    vfs_super_block_unref(sb);
    return status;
}

errno_t vfs_sync_all() {
    // Take a snapshot so we don't call into file systems while holding a lock.
    mutex_lock(&supers_lock);
    size_t count = vec_length(&mounted_supers);
    struct vfs_super_block** supers = count ? mem_alloc(count * sizeof(*supers), ALLOC_NOZERO) : nullptr;
    if (count && !supers) {
        mutex_unlock(&supers_lock);
        return ENOMEM;
    }
    for (size_t i = 0; i < count; i++) {
        supers[i] = vec_elem_get(&mounted_supers, i);
        vfs_super_block_ref(supers[i]);
    }
    mutex_unlock(&supers_lock);

    errno_t result = 0;
    for (size_t i = 0; i < count; i++) {
        struct vfs_super_block* sb = supers[i];
        if (sb->ops && sb->ops->sync) {
            errno_t status = sb->ops->sync(sb);
            if (status)
                result = status;
        }
        vfs_super_block_unref(sb);
    }

    mem_free(supers, count * sizeof(*supers));
    return result;
}

errno_t vfs_mount_new(enum mount_flags flags, struct vfs_entry* root, struct vfs_mount** out) {
    struct vfs_mount* mount = mem_alloc(sizeof(*mount), 0);
    if (!mount)
        return ENOMEM;

    refcount_init(&mount->rc);
    mount->flags = flags;
    mount->root = root;
    vfs_entry_ref(root);
    mount->has_mount_point = false;

    *out = mount;
    return 0;
}

errno_t vfs_path_mount(struct vfs_path at, struct vfs_mount* mount) {
    if (mount->has_mount_point)
        return EBUSY;

    mutex_lock(&at.entry->lock);
    errno_t status = vec_push(&at.entry->mounts, mount);
    mutex_unlock(&at.entry->lock);
    if (status)
        return status;

    vfs_mount_ref(mount);
    mount->mount_point = vfs_path_clone(at);
    mount->has_mount_point = true;
    return 0;
}

errno_t vfs_path_unmount(struct vfs_path path) {
    struct vfs_mount* mount = path.mount;
    if (path.entry != mount->root || !mount->has_mount_point)
        return EINVAL;

    vfs_entry_prune(mount->root);
    mutex_lock(&mount->root->lock);
    const bool busy = !tailq_empty(&mount->root->children);
    mutex_unlock(&mount->root->lock);
    if (busy)
        return EBUSY;

    struct vfs_entry* point = mount->mount_point.entry;
    bool found = false;
    mutex_lock(&point->lock);
    vec_foreach(&point->mounts, i) {
        if (vec_elem_get(&point->mounts, i) == mount) {
            vec_remove(&point->mounts, i);
            found = true;
            break;
        }
    }
    mutex_unlock(&point->lock);
    if (!found)
        return EINVAL;

    struct vfs_path mount_point = mount->mount_point;
    mount->mount_point = (struct vfs_path){};
    mount->has_mount_point = false;
    vfs_path_drop(mount_point);

    errno_t status = vfs_unmount(mount);
    vfs_mount_unref(mount);
    return status;
}
