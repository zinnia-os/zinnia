// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "ext2: " fmt

#include <dev/char/chardev.h>
#include <fs/ext2/ext2.h>
#include <lib/endian.h>
#include <lib/print.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <mem/vma.h>
#include <string.h>

static void inode_to_cpu(struct ext2_inode* dst, const struct ext2_inode* src) {
    *dst = (struct ext2_inode){
        .i_mode = le16_to_cpu(src->i_mode),
        .i_uid = le16_to_cpu(src->i_uid),
        .i_size = le32_to_cpu(src->i_size),
        .i_atime = le32_to_cpu(src->i_atime),
        .i_ctime = le32_to_cpu(src->i_ctime),
        .i_mtime = le32_to_cpu(src->i_mtime),
        .i_dtime = le32_to_cpu(src->i_dtime),
        .i_gid = le16_to_cpu(src->i_gid),
        .i_links_count = le16_to_cpu(src->i_links_count),
        .i_blocks = le32_to_cpu(src->i_blocks),
        .i_flags = le32_to_cpu(src->i_flags),
        .i_osd1 = le32_to_cpu(src->i_osd1),
        .i_generation = le32_to_cpu(src->i_generation),
        .i_file_acl = le32_to_cpu(src->i_file_acl),
        .i_size_high = le32_to_cpu(src->i_size_high),
        .i_faddr = le32_to_cpu(src->i_faddr),
        .i_frag = src->i_frag,
        .i_fsize = src->i_fsize,
        .i_pad1 = le16_to_cpu(src->i_pad1),
        .i_uid_high = le16_to_cpu(src->i_uid_high),
        .i_gid_high = le16_to_cpu(src->i_gid_high),
        .i_reserved2 = le32_to_cpu(src->i_reserved2),
    };

    for (size_t i = 0; i < EXT2_N_BLOCKS; i++)
        dst->i_block[i] = le32_to_cpu(src->i_block[i]);
}

static errno_t inode_location(struct ext2_super* sb, uint32_t ino, uint64_t* blkno, size_t* offset) {
    if (ino == 0 || ino > sb->inodes_count)
        return EUCLEAN;

    // Inode numbers start at 1.
    const uint32_t group = ext2_group_of_ino(sb, ino);
    const uint64_t byte = (uint64_t)((ino - 1) % sb->inodes_per_group) * sb->inode_size;

    *blkno = sb->groups[group].inode_table + byte / sb->block_size;
    *offset = byte % sb->block_size;
    return 0;
}

errno_t ext2_read_inode(struct ext2_super* sb, uint32_t ino, struct ext2_inode* out) {
    uint64_t blkno;
    size_t offset;
    errno_t status = inode_location(sb, ino, &blkno, &offset);
    if (status)
        return status;

    struct buf* buf;
    status = buf_read(sb->dev, blkno, sb->block_size, &buf);
    if (status)
        return status;

    inode_to_cpu(out, (struct ext2_inode*)((uint8_t*)buf->data + offset));
    buf_drop(buf);
    return 0;
}

// TODO: ext2_write_inode, ext2_store, ext2_set_size, ext2_touch.

void ext2_update_vnode(struct vfs_node* node, struct ext2_inode_info* info) {
    const struct ext2_inode* raw = &info->raw;

    mutex_lock(&node->lock);
    node->size = ext2_size(raw);
    node->blocks = raw->i_blocks;
    node->nlink = raw->i_links_count;
    node->mode = raw->i_mode & 07777;
    node->uid = ext2_uid(raw);
    node->gid = ext2_gid(raw);
    node->atime = time_from_sec(raw->i_atime);
    node->mtime = time_from_sec(raw->i_mtime);
    node->ctime = time_from_sec(raw->i_ctime);
    mutex_unlock(&node->lock);
}

bool ext2_has_blocks(struct ext2_inode_info* info) {
    const struct ext2_inode* raw = &info->raw;

    switch (raw->i_mode & __S_IFMT) {
    case __S_IFREG:
    case __S_IFDIR:
        return true;
    case __S_IFLNK: {
        // Fast symlinks own no blocks, but an ACL block still counts in `i_blocks`.
        const uint32_t acl_sectors = raw->i_file_acl ? info->sb->block_size / EXT2_SECTOR_SIZE : 0;
        return raw->i_blocks > acl_sectors;
    }
    default:
        return false;
    }
}

// Splits `lblk` into one index per level of the block map and returns the depth.
static size_t block_path(struct ext2_super* sb, uint64_t lblk, uint32_t offsets[4]) {
    const uint64_t ptrs = sb->block_size / sizeof(uint32_t);

    if (lblk < EXT2_NDIR_BLOCKS) {
        offsets[0] = lblk;
        return 1;
    }

    lblk -= EXT2_NDIR_BLOCKS;
    if (lblk < ptrs) {
        offsets[0] = EXT2_IND_BLOCK;
        offsets[1] = lblk;
        return 2;
    }

    lblk -= ptrs;
    if (lblk < ptrs * ptrs) {
        offsets[0] = EXT2_DIND_BLOCK;
        offsets[1] = lblk / ptrs;
        offsets[2] = lblk % ptrs;
        return 3;
    }

    lblk -= ptrs * ptrs;
    if (lblk < ptrs * ptrs * ptrs) {
        offsets[0] = EXT2_TIND_BLOCK;
        offsets[1] = lblk / (ptrs * ptrs);
        offsets[2] = (lblk / ptrs) % ptrs;
        offsets[3] = lblk % ptrs;
        return 4;
    }

    return 0;
}

// TODO: Block allocation in ext2_bmap.
errno_t ext2_bmap(struct ext2_inode_info* info, uint64_t lblk, uint32_t* out) {
    struct ext2_super* sb = info->sb;

    uint32_t offsets[4];
    const size_t depth = block_path(sb, lblk, offsets);
    if (depth == 0)
        return EFBIG;

    // A zero entry at any level is a hole.
    uint32_t block = info->raw.i_block[offsets[0]];
    if (!block) {
        *out = 0;
        return 0;
    }
    if (!ext2_valid_block(sb, block))
        return EUCLEAN;

    for (size_t level = 1; level < depth; level++) {
        struct buf* buf;
        errno_t status = buf_read(sb->dev, block, sb->block_size, &buf);
        if (status)
            return status;

        const uint32_t next = le32_to_cpu(((uint32_t*)buf->data)[offsets[level]]);
        buf_drop(buf);

        if (!next) {
            *out = 0;
            return 0;
        }
        if (!ext2_valid_block(sb, next))
            return EUCLEAN;

        block = next;
    }

    *out = block;
    return 0;
}

// TODO: ext2_truncate_blocks.

struct extent {
    struct ext2_super* sb;
    enum bio_op op;
    uint32_t start;
    size_t blocks;
    struct bio_seg* segs;
    size_t num_segs;
};

static errno_t extent_flush(struct extent* ext) {
    if (ext->blocks == 0)
        return 0;

    struct ext2_super* sb = ext->sb;
    const size_t per_block = sb->block_size / sb->dev->lba_size;
    const size_t num_lbas = ext->blocks * per_block;

    size_t done = 0;
    errno_t status =
        blockdev_rw(sb->dev, ext->op, (uint64_t)ext->start * per_block, num_lbas, ext->segs, ext->num_segs, &done);

    ext->blocks = 0;
    ext->num_segs = 0;

    if (!status && done < num_lbas)
        status = EIO;
    return status;
}

static errno_t extent_add(struct extent* ext, uint32_t block, phys_t addr) {
    const size_t len = ext->sb->block_size;

    // The block continues the run, so we grow it instead of flushing.
    if (ext->blocks && block == ext->start + ext->blocks) {
        struct bio_seg* last = &ext->segs[ext->num_segs - 1];
        if (last->addr + last->len == addr)
            last->len += len;
        else
            ext->segs[ext->num_segs++] = (struct bio_seg){.addr = addr, .len = len};

        ext->blocks++;
        return 0;
    }

    errno_t status = extent_flush(ext);

    ext->start = block;
    ext->blocks = 1;
    ext->segs[0] = (struct bio_seg){.addr = addr, .len = len};
    ext->num_segs = 1;
    return status;
}

static errno_t info_get_pages(struct vmo* vmo, uint64_t index, struct page** pages, size_t count, size_t* out_valid) {
    struct ext2_inode_info* info = containerof(vmo, struct ext2_inode_info, vmo);
    struct ext2_super* sb = info->sb;
    const size_t pgsz = page_size();
    const size_t per_page = pgsz / sb->block_size;

    rwlock_read_lock(&info->lock);

    // Only pages that overlap the file get filled.
    const uint64_t size = ext2_size(&info->raw);
    const uint64_t limit = size / pgsz + (size % pgsz != 0);
    const size_t valid = index >= limit ? 0 : MIN(count, limit - index);

    // Holes are never read, so they stay zero.
    for (size_t i = 0; i < valid; i++)
        memset(HHDM_PTR(page_addr(pages[i])), 0, pgsz);

    errno_t status = 0;
    struct extent ext = {.sb = sb, .op = BIO_READ};
    if (valid) {
        ext.segs = mem_alloc(valid * per_page * sizeof(struct bio_seg), ALLOC_NOZERO);
        if (!ext.segs)
            status = ENOMEM;
    }

    for (size_t b = 0; b < valid * per_page && !status; b++) {
        const uint64_t lblk = index * per_page + b;
        if (lblk * sb->block_size >= size)
            break;

        uint32_t block;
        status = ext2_bmap(info, lblk, &block);
        if (status)
            break;

        if (!block) {
            status = extent_flush(&ext);
            continue;
        }

        status = extent_add(&ext, block, page_addr(pages[b / per_page]) + (b % per_page) * sb->block_size);
    }

    if (!status)
        status = extent_flush(&ext);

    rwlock_read_unlock(&info->lock);
    mem_free(ext.segs, valid * per_page * sizeof(struct bio_seg));

    // The last block could hold stale data past EOF.
    if (!status && valid && valid == limit - index && size % pgsz != 0)
        memset((uint8_t*)HHDM_PTR(page_addr(pages[valid - 1])) + size % pgsz, 0, pgsz - size % pgsz);

    *out_valid = status ? 0 : valid;
    return status;
}

// TODO: info_put_pages and orphan eviction.

static void info_destroy(struct vmo* vmo) {
    struct ext2_inode_info* info = containerof(vmo, struct ext2_inode_info, vmo);
    struct ext2_super* sb = info->sb;

    // Lookups that found this dying entry wait for it to leave the cache.
    mutex_lock(&sb->icache_lock);
    if (radix_lookup(&sb->icache, info->ino) == info)
        radix_remove(&sb->icache, info->ino);
    condvar_broadcast(&sb->icache_cv);
    mutex_unlock(&sb->icache_lock);

    vfs_super_block_unref(&sb->base);
    mem_free(info, sizeof(*info));
}

static const struct vmo_pager_ops ext2_pager = {
    .get_pages = info_get_pages,
    .max_cluster = 16,
    .destroy = info_destroy,
};

static errno_t new_info(
    struct ext2_super* sb,
    uint32_t ino,
    const struct ext2_inode* raw,
    struct ext2_inode_info** out
) {
    struct ext2_inode_info* info = mem_alloc(sizeof(*info), 0);
    if (!info)
        return ENOMEM;

    vmo_init(&info->vmo, &ext2_pager);
    rwlock_init(&info->lock);

    info->sb = sb;
    vfs_super_block_ref(&sb->base);
    info->ino = ino;
    info->raw = *raw;

    *out = info;
    return 0;
}

static void discard_info(struct ext2_inode_info* info) {
    vfs_super_block_unref(&info->sb->base);
    mem_free(info, sizeof(*info));
}

// The old encoding lives in `i_block[0]`, the new one in `i_block[1]`.
static dev_t decode_rdev(const struct ext2_inode* raw) {
    if (raw->i_block[0]) {
        const uint32_t old = raw->i_block[0];
        return dev_make((old >> 8) & 0xFF, old & 0xFF);
    }

    const uint32_t new = raw->i_block[1];
    return dev_make((new & 0xFFF00) >> 8, (new & 0xFF) | ((new >> 12) & 0xFFF00));
}

static errno_t node_kind(mode_t mode, enum vfs_node_type* type, const struct vfs_node_ops** ops) {
    switch (mode & __S_IFMT) {
    case __S_IFREG:
        *type = VNODE_REGULAR;
        *ops = &ext2_regular_ops;
        return 0;
    case __S_IFDIR:
        *type = VNODE_DIRECTORY;
        *ops = &ext2_dir_ops;
        return 0;
    case __S_IFLNK:
        *type = VNODE_SYMLINK;
        *ops = &ext2_symlink_ops;
        return 0;
    case __S_IFIFO:
        *type = VNODE_FIFO;
        *ops = &ext2_special_ops;
        return 0;
    case __S_IFCHR:
        *type = VNODE_CHARDEV;
        *ops = &ext2_special_ops;
        return 0;
    case __S_IFBLK:
        *type = VNODE_BLOCKDEV;
        *ops = &ext2_special_ops;
        return 0;
    case __S_IFSOCK:
        *type = VNODE_SOCKET;
        *ops = &ext2_special_ops;
        return 0;
    default:
        return EUCLEAN;
    }
}

// Consumes the caller's reference to `info`, which the new vnode takes over.
static errno_t attach_vnode(struct ext2_inode_info* info, struct vfs_node** out) {
    struct ext2_super* sb = info->sb;

    rwlock_read_lock(&info->lock);

    enum vfs_node_type type;
    const struct vfs_node_ops* ops;
    errno_t status = node_kind(info->raw.i_mode, &type, &ops);

    struct vfs_node* node = nullptr;
    if (!status)
        status = vfs_node_new(&sb->base, type, ops, info->raw.i_mode, &node);

    if (!status) {
        node->priv = info;
        node->id = info->ino;
        if (type == VNODE_CHARDEV || type == VNODE_BLOCKDEV)
            node->rdev = decode_rdev(&info->raw);
        ext2_update_vnode(node, info);
    }

    rwlock_read_unlock(&info->lock);

    if (status) {
        vmo_unref(&info->vmo);
        return status;
    }

    // Another thread may have attached a vnode in the meantime.
    mutex_lock(&sb->icache_lock);
    struct vfs_node* existing = info->vnode;
    if (existing && refcount_inc_not_zero(&existing->rc)) {
        mutex_unlock(&sb->icache_lock);
        vfs_node_unref(node);
        *out = existing;
        return 0;
    }
    info->vnode = node;
    mutex_unlock(&sb->icache_lock);

    *out = node;
    return 0;
}

errno_t ext2_get_node(struct ext2_super* sb, uint32_t ino, struct vfs_node** out) {
    if (ino == 0 || ino > sb->inodes_count)
        return EUCLEAN;

    while (true) {
        mutex_lock(&sb->icache_lock);

        // An entry with no references is being destroyed. Wait for it to go.
        struct ext2_inode_info* info;
        while ((info = radix_lookup(&sb->icache, ino)) && !refcount_inc_not_zero(&info->vmo.rc))
            condvar_wait(&sb->icache_cv, &sb->icache_lock);

        if (info) {
            struct vfs_node* node = info->vnode;
            if (node && refcount_inc_not_zero(&node->rc)) {
                mutex_unlock(&sb->icache_lock);
                vmo_unref(&info->vmo);
                *out = node;
                return 0;
            }

            mutex_unlock(&sb->icache_lock);
            return attach_vnode(info, out);
        }

        mutex_unlock(&sb->icache_lock);

        struct ext2_inode raw;
        errno_t status = ext2_read_inode(sb, ino, &raw);
        if (status)
            return status;

        if (raw.i_links_count == 0) {
            log_warn("Reference to deleted inode %u\n", ino);
            return EUCLEAN;
        }

        status = new_info(sb, ino, &raw, &info);
        if (status)
            return status;

        // Someone else read the same inode while we didn't hold the lock.
        mutex_lock(&sb->icache_lock);
        if (radix_lookup(&sb->icache, ino)) {
            mutex_unlock(&sb->icache_lock);
            discard_info(info);
            continue;
        }

        status = radix_insert(&sb->icache, ino, info);
        mutex_unlock(&sb->icache_lock);

        if (status) {
            discard_info(info);
            return status;
        }

        return attach_vnode(info, out);
    }
}

// TODO: ext2_new_node, ext2_sync_inodes, ext2_node_sync.

void ext2_node_destroy(struct vfs_node* node) {
    struct ext2_inode_info* info = ext2_info(node);
    struct ext2_super* sb = info->sb;

    mutex_lock(&sb->icache_lock);
    if (info->vnode == node)
        info->vnode = nullptr;
    mutex_unlock(&sb->icache_lock);

    vmo_unref(&info->vmo);
}

static errno_t ext2_regular_read(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    struct vfs_node* node = file->vnode;

    mutex_lock(&node->lock);
    const uint64_t size = node->size;
    mutex_unlock(&node->lock);

    *out_read = 0;
    if (offset >= size)
        return 0;

    size_t progress;
    errno_t status =
        vmo_read_iter(&ext2_info(node)->vmo, offset, buffer, MIN(iovec_iter_len(buffer), size - offset), &progress);

    // A short read still succeeds.
    *out_read = progress;
    return progress > 0 ? 0 : status;
}

// TODO: Regular file write, truncate and sync.

static errno_t ext2_regular_get_object(struct vfs_node* node, struct vmo** out) {
    struct vmo* obj = &ext2_info(node)->vmo;
    vmo_ref(obj);
    *out = obj;
    return 0;
}

static errno_t ext2_regular_mmap(
    struct vfs_file* file,
    struct vma* vma,
    uintptr_t addr,
    size_t len,
    enum prot_flags prot,
    enum map_flags flags,
    off_t offset,
    uintptr_t* out_addr
) {
    // Writes through a shared mapping would reach the disk.
    if ((flags & MAP_SHARED) && (prot & PROT_WRITE) && !ext2_info(file->vnode)->sb->writable)
        return EACCES;

    enum vma_flags vflags = (flags & MAP_SHARED) ? VMA_SHARED : 0;
    errno_t status = vma_map(vma, &ext2_info(file->vnode)->vmo, addr, len, prot, vflags, offset);
    if (status)
        return status;

    *out_addr = addr;
    return 0;
}

static const struct vfs_file_ops ext2_regular_file_ops = {
    .read = ext2_regular_read,
    .mmap = ext2_regular_mmap,
};

const struct vfs_node_ops ext2_regular_ops = {
    .destroy = ext2_node_destroy,
    .file_ops = &ext2_regular_file_ops,
    .regular = {
        .get_vmo = ext2_regular_get_object,
    },
};

static errno_t ext2_symlink_read_link(struct vfs_node* node, iovec_iter_t* iter, size_t* out_read) {
    struct ext2_inode_info* info = ext2_info(node);
    struct ext2_super* sb = info->sb;

    rwlock_read_lock(&info->lock);

    const uint64_t size = ext2_size(&info->raw);
    if (size > sb->block_size) {
        rwlock_read_unlock(&info->lock);
        return EUCLEAN;
    }

    char* target = mem_alloc(size + 1, ALLOC_NOZERO);
    if (!target) {
        rwlock_read_unlock(&info->lock);
        return ENOMEM;
    }

    errno_t status = 0;
    if (!ext2_has_blocks(info)) {
        // Fast symlinks keep the target in `i_block`, which we already swapped to CPU order.
        if (size > EXT2_FAST_SYMLINK_MAX) {
            status = EUCLEAN;
        } else {
            uint32_t inline_data[EXT2_N_BLOCKS];
            for (size_t i = 0; i < EXT2_N_BLOCKS; i++)
                inline_data[i] = cpu_to_le32(info->raw.i_block[i]);
            memcpy(target, inline_data, size);
        }
    } else {
        uint32_t block;
        status = ext2_bmap(info, 0, &block);
        if (!status && !block)
            status = EUCLEAN;

        struct buf* buf;
        if (!status)
            status = buf_read(sb->dev, block, sb->block_size, &buf);
        if (!status) {
            memcpy(target, buf->data, size);
            buf_drop(buf);
        }
    }

    rwlock_read_unlock(&info->lock);

    if (!status)
        status = iovec_iter_copy_to(iter, target, size, out_read);

    mem_free(target, size + 1);
    return status;
}

static const struct vfs_file_ops ext2_symlink_file_ops = {};

const struct vfs_node_ops ext2_symlink_ops = {
    .destroy = ext2_node_destroy,
    .file_ops = &ext2_symlink_file_ops,
    .symlink = {
        .read_link = ext2_symlink_read_link,
    },
};

const struct vfs_node_ops ext2_special_ops = {
    .destroy = ext2_node_destroy,
};
