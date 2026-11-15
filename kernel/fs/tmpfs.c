// SPDX-License-Identifier: BSD-3-Clause

#include <arch/mem.h>
#include <fs/vfs.h>
#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/clock.h>
#include <lib/initgraph.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <mem/vma.h>
#include <mem/vmo.h>
#include <string.h>

struct tmpfs_super {
    struct vfs_super_block base;
    _Atomic(ino_t) inode_counter;
};

struct tmpfs_symlink {
    char* target;
    size_t len;
};

struct tmpfs_file {
    struct vmo vmo;
    _Atomic(uint64_t) size;
};

static errno_t tmpfs_file_get_pages(
    struct vmo* vmo,
    uint64_t index,
    struct page** pages,
    size_t count,
    size_t* out_valid
) {
    struct tmpfs_file* file = containerof(vmo, struct tmpfs_file, vmo);
    const size_t page_size = arch_mem_page_size();
    const uint64_t size = atomic_load(&file->size, ATOMIC_ACQUIRE);
    const uint64_t limit = size / page_size + (size % page_size != 0);

    const size_t valid = index >= limit ? 0 : MIN(count, limit - index);
    for (size_t i = 0; i < valid; i++)
        memset(HHDM_PTR(page_addr(pages[i])), 0, page_size);
    *out_valid = valid;
    return 0;
}

static void tmpfs_file_destroy(struct vmo* vmo) {
    mem_free(containerof(vmo, struct tmpfs_file, vmo), sizeof(struct tmpfs_file));
}

static const struct vmo_pager_ops tmpfs_file_pager = {
    .get_pages = tmpfs_file_get_pages,
    .destroy = tmpfs_file_destroy,
};

static const struct vfs_node_ops tmpfs_dir_ops;
static const struct vfs_node_ops tmpfs_regular_ops;
static const struct vfs_node_ops tmpfs_symlink_ops;

static errno_t tmpfs_create_inode(
    struct vfs_super_block* sb,
    enum vfs_node_type type,
    const struct vfs_node_ops* ops,
    mode_t mode,
    const struct ident* identity,
    struct vfs_node** out
) {
    struct tmpfs_super* super = containerof(sb, struct tmpfs_super, base);

    struct vfs_node* node;
    errno_t status = vfs_node_new(sb, type, ops, mode, &node);
    if (status)
        return status;

    node->id = atomic_fetch_add(&super->inode_counter, 1, ATOMIC_RELAXED);
    node->uid = identity->euid;
    node->gid = identity->egid;

    *out = node;
    return 0;
}

// Super block

static errno_t tmpfs_super_sync(struct vfs_super_block* sb) {
    // This is a no-op.
    return 0;
}

static errno_t tmpfs_super_statvfs(struct vfs_super_block* sb, struct __statvfs* out) {
    memset(out, 0, sizeof(*out));
    out->f_namemax = __NAME_MAX;
    return 0;
}

static void tmpfs_super_destroy(struct vfs_super_block* sb) {
    mem_free(containerof(sb, struct tmpfs_super, base), sizeof(struct tmpfs_super));
}

static const struct vfs_super_block_ops tmpfs_super_ops = {
    .sync = tmpfs_super_sync,
    .statvfs = tmpfs_super_statvfs,
    .destroy = tmpfs_super_destroy,
};

// Directories

static errno_t tmpfs_dir_lookup(struct vfs_node* node, struct vfs_path* child) {
    // tmpfs directories only live in memory, so we cannot look up entries that do not exist.
    return ENOENT;
}

static errno_t tmpfs_dir_open(
    struct vfs_node* node,
    struct vfs_path* path,
    enum open_flags flags,
    const struct ident* identity,
    struct vfs_file** out_file
) {
    return vfs_file_new(node, path, flags, out_file);
}

static errno_t tmpfs_dir_create(
    struct vfs_node* node,
    struct vfs_entry* entry,
    mode_t mode,
    const struct ident* identity
) {
    struct tmpfs_file* data = mem_alloc(sizeof(struct tmpfs_file), 0);
    if (!data)
        return ENOMEM;
    vmo_init(&data->vmo, &tmpfs_file_pager);
    struct vmo* cache = &data->vmo;
    errno_t status;

    struct vfs_node* file;
    status = tmpfs_create_inode(node->sb, VNODE_REGULAR, &tmpfs_regular_ops, mode, identity, &file);
    if (status) {
        vmo_unref(cache);
        return status;
    }

    // Attach the cache before the node becomes visible through the entry.
    file->priv = cache;
    vfs_entry_set_inode(entry, file);
    vfs_node_unref(file);
    return 0;
}

static errno_t tmpfs_dir_mkdir(
    struct vfs_node* node,
    struct vfs_path* path,
    mode_t mode,
    const struct ident* identity
) {
    struct vfs_node* dir;
    errno_t status = tmpfs_create_inode(node->sb, VNODE_DIRECTORY, &tmpfs_dir_ops, mode, identity, &dir);
    if (status)
        return status;

    vfs_entry_set_inode(path->entry, dir);
    vfs_node_unref(dir);
    return 0;
}

static errno_t tmpfs_dir_symlink(
    struct vfs_node* node,
    struct vfs_path* path,
    const char* target,
    size_t target_len,
    const struct ident* identity
) {
    struct tmpfs_symlink* link = mem_alloc(sizeof(*link), 0);
    if (!link)
        return ENOMEM;
    link->target = mem_alloc(target_len + 1, ALLOC_NOZERO);
    if (!link->target) {
        mem_free(link, sizeof(*link));
        return ENOMEM;
    }
    memcpy(link->target, target, target_len);
    link->target[target_len] = '\0';
    link->len = target_len;

    struct vfs_node* sym;
    errno_t status = tmpfs_create_inode(node->sb, VNODE_SYMLINK, &tmpfs_symlink_ops, 0777, identity, &sym);
    if (status) {
        mem_free(link->target, link->len + 1);
        mem_free(link, sizeof(*link));
        return status;
    }

    sym->priv = link;
    sym->size = target_len;
    vfs_entry_set_inode(path->entry, sym);
    vfs_node_unref(sym);
    return 0;
}

static void adjust_links(struct vfs_node* node, int delta) {
    mutex_lock(&node->lock);
    node->nlink += delta;
    node->ctime = time_from_nano(clock_get_realtime_ns());
    mutex_unlock(&node->lock);
}

static errno_t tmpfs_dir_link(
    struct vfs_node* node,
    struct vfs_path* path,
    struct vfs_node* target,
    const struct ident* identity
) {
    adjust_links(target, 1);
    vfs_entry_set_inode(path->entry, target);
    return 0;
}

// Removes `child` from the cached children of `parent` and drops the list's reference.
static void detach_child(struct vfs_entry* parent, struct vfs_entry* child) {
    bool found = false;

    mutex_lock(&parent->lock);
    for (struct vfs_entry* it = tailq_first(&parent->children); it; it = tailq_next(it, child_link)) {
        if (it == child) {
            tailq_remove(&parent->children, child, child_link);
            found = true;
            break;
        }
    }
    mutex_unlock(&parent->lock);

    if (found)
        vfs_entry_unref(child);
}

static errno_t tmpfs_dir_unlink(struct vfs_node* node, struct vfs_path* path, const struct ident* identity) {
    struct vfs_path parent;
    errno_t status = vfs_path_lookup_parent(*path, &parent);
    if (status)
        return status;

    struct vfs_node* victim = vfs_entry_get_inode(path->entry);
    if (victim) {
        adjust_links(victim, -1);
        vfs_node_unref(victim);
    }
    vfs_entry_set_negative(path->entry);
    detach_child(parent.entry, path->entry);
    vfs_path_drop(parent);
    return 0;
}

static errno_t tmpfs_dir_rename(
    struct vfs_node* node,
    struct vfs_path* path,
    struct vfs_node* target,
    struct vfs_path* target_path,
    const struct ident* identity
) {
    struct vfs_node* moved = vfs_entry_get_inode(path->entry);
    if (!moved)
        return ENOENT;

    struct vfs_path parent;
    errno_t status = vfs_path_lookup_parent(*path, &parent);
    if (status) {
        vfs_node_unref(moved);
        return status;
    }

    struct vfs_node* replaced = vfs_entry_get_inode(target_path->entry);
    if (replaced) {
        adjust_links(replaced, -1);
        vfs_node_unref(replaced);
    }
    vfs_entry_set_inode(target_path->entry, moved);
    vfs_node_unref(moved);

    // Transfer children for directory renames.
    struct vfs_entry* src = path->entry;
    vfs_entry_replace_children(target_path->entry, src);

    vfs_entry_set_negative(src);
    detach_child(parent.entry, src);
    vfs_path_drop(parent);
    return 0;
}

static errno_t tmpfs_dir_mknod(
    struct vfs_node* node,
    struct vfs_path* path,
    mode_t mode,
    struct vfs_mknod_target* target,
    const struct ident* identity
) {
    if (!target)
        return ENODEV;

    struct vfs_node* new_node;
    errno_t status = tmpfs_create_inode(node->sb, target->type, target->ops, mode, identity, &new_node);
    if (status)
        return status;

    new_node->priv = target->priv;
    new_node->rdev = target->rdev;
    vfs_entry_set_inode(path->entry, new_node);
    vfs_node_unref(new_node);
    return 0;
}

static const struct vfs_file_ops tmpfs_dir_file_ops = {};

static const struct vfs_node_ops tmpfs_dir_ops = {
    .file_ops = &tmpfs_dir_file_ops,
    .dir = {
        .lookup = tmpfs_dir_lookup,
        .open = tmpfs_dir_open,
        .create = tmpfs_dir_create,
        .mkdir = tmpfs_dir_mkdir,
        .symlink = tmpfs_dir_symlink,
        .link = tmpfs_dir_link,
        .unlink = tmpfs_dir_unlink,
        .rmdir = tmpfs_dir_unlink,
        .rename = tmpfs_dir_rename,
        .mknod = tmpfs_dir_mknod,
        .get_dir_entries = vfs_dir_get_cached_entries,
    },
};

// Regular files

static struct vmo* regular_cache(struct vfs_node* node) {
    return node->priv;
}

static void set_size(struct vfs_node* node, uint64_t size) {
    node->size = size;
    atomic_store(&containerof(regular_cache(node), struct tmpfs_file, vmo)->size, size, ATOMIC_RELEASE);
}

static errno_t tmpfs_regular_read(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    struct vfs_node* node = file->vnode;

    mutex_lock(&node->lock);
    size_t size = node->size;
    mutex_unlock(&node->lock);

    *out_read = 0;
    if (offset >= size)
        return 0;

    size_t progress;
    errno_t status =
        vmo_read_iter(regular_cache(node), offset, buffer, MIN(iovec_iter_len(buffer), size - offset), &progress);
    *out_read = progress;
    return progress > 0 ? 0 : status;
}

static errno_t tmpfs_regular_write(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written) {
    struct vfs_node* node = file->vnode;
    const size_t total = iovec_iter_len(buffer);

    mutex_lock(&node->lock);
    const uint64_t old_size = node->size;
    if (offset + total > old_size)
        set_size(node, offset + total);
    size_t progress;
    errno_t status = vmo_write_iter(regular_cache(node), offset, buffer, total, &progress);
    if (progress == 0 && total > 0 && !status)
        status = ENOMEM;
    if (offset + total > old_size)
        set_size(node, MAX(old_size, offset + progress));
    mutex_unlock(&node->lock);

    *out_written = progress;
    return progress > 0 ? 0 : status;
}

static errno_t tmpfs_regular_mmap(
    struct vfs_file* file,
    struct vma* vma,
    uintptr_t addr,
    size_t len,
    enum prot_flags prot,
    enum map_flags flags,
    off_t offset,
    uintptr_t* out_addr
) {
    enum vma_flags vflags = (flags & MAP_SHARED) ? VMA_SHARED : 0;
    errno_t status = vma_map(vma, regular_cache(file->vnode), addr, len, prot, vflags, offset);
    if (status)
        return status;

    *out_addr = addr;
    return 0;
}

static errno_t tmpfs_regular_truncate(struct vfs_node* node, uint64_t new_length) {
    mutex_lock(&node->lock);
    const uint64_t old_size = node->size;
    set_size(node, new_length);
    if (new_length < old_size)
        vmo_truncate(regular_cache(node), new_length);
    mutex_unlock(&node->lock);
    return 0;
}

static errno_t tmpfs_regular_get_object(struct vfs_node* node, struct vmo** out) {
    struct vmo* obj = regular_cache(node);
    vmo_ref(obj);
    *out = obj;
    return 0;
}

static void tmpfs_regular_destroy(struct vfs_node* node) {
    if (node->priv)
        vmo_unref(regular_cache(node));
}

static const struct vfs_file_ops tmpfs_regular_file_ops = {
    .read = tmpfs_regular_read,
    .write = tmpfs_regular_write,
    .mmap = tmpfs_regular_mmap,
};

static const struct vfs_node_ops tmpfs_regular_ops = {
    .destroy = tmpfs_regular_destroy,
    .file_ops = &tmpfs_regular_file_ops,
    .regular = {
        .truncate = tmpfs_regular_truncate,
        .get_vmo = tmpfs_regular_get_object,
    },
};

// Symbolic links

static errno_t tmpfs_symlink_read_link(struct vfs_node* node, iovec_iter_t* iter, size_t* out_read) {
    struct tmpfs_symlink* link = node->priv;
    return iovec_iter_copy_to(iter, link->target, link->len, out_read);
}

static void tmpfs_symlink_destroy(struct vfs_node* node) {
    struct tmpfs_symlink* link = node->priv;
    if (link) {
        mem_free(link->target, link->len + 1);
        mem_free(link, sizeof(*link));
    }
}

static const struct vfs_file_ops tmpfs_symlink_file_ops = {};

static const struct vfs_node_ops tmpfs_symlink_ops = {
    .destroy = tmpfs_symlink_destroy,
    .file_ops = &tmpfs_symlink_file_ops,
    .symlink = {
        .read_link = tmpfs_symlink_read_link,
    },
};

// File system

static errno_t tmpfs_mount(const struct vfs_filesystem* fs, enum mount_flags flags, void* arg, struct vfs_mount** out) {
    struct tmpfs_super* super = mem_alloc(sizeof(*super), 0);
    if (!super)
        return ENOMEM;
    refcount_init(&super->base.rc);
    super->base.ops = &tmpfs_super_ops;
    super->base.flags = flags;
    atomic_init(&super->inode_counter, 1);

    struct vfs_node* root_node = nullptr;
    struct vfs_entry* root_entry = nullptr;

    errno_t status =
        tmpfs_create_inode(&super->base, VNODE_DIRECTORY, &tmpfs_dir_ops, 01777, ident_kernel(), &root_node);
    if (status)
        goto out;

    status = vfs_entry_new("", 0, nullptr, &root_entry);
    if (status)
        goto out;
    vfs_entry_set_inode(root_entry, root_node);

    status = vfs_mount_new(flags, root_entry, out);

out:
    if (root_entry)
        vfs_entry_unref(root_entry);
    if (root_node)
        vfs_node_unref(root_node);
    // The root inode keeps the super block alive now.
    vfs_super_block_unref(&super->base);
    return status;
}

static const struct vfs_filesystem tmpfs = {
    .name = "tmpfs",
    .mount = tmpfs_mount,
};

static void tmpfs_stage_init(struct boot_info* info) {
    errno_t status = vfs_register_filesystem(&tmpfs);
    assert(status == 0, "Unable to register tmpfs (%i)", status);
}

INIT_TASK(tmpfs_stage, "fs.tmpfs", tmpfs_stage_init);
INIT_DEPENDS(tmpfs_stage, mem_stage);
INIT_ENTAILS(tmpfs_stage, vfs_stage);
