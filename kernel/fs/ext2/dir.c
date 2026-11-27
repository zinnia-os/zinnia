// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "ext2: " fmt

#include <fs/ext2/ext2.h>
#include <lib/endian.h>
#include <lib/print.h>
#include <lib/utils.h>
#include <string.h>

constexpr size_t DIRENT_HEADER = sizeof(struct ext2_dir_entry);

struct dir_iter {
    struct ext2_inode_info* dir;
    uint64_t lblk;       // Current logical block.
    uint64_t num_blocks; // Blocks covered by the directory size.
    uint32_t block;      // Device block of `lblk`.
    struct buf* buf;     // Held while inside a block.
    size_t next;         // Offset of the next entry.
    size_t offset;       // Offset of the last returned entry.
};

static void dir_iter_finish(struct dir_iter* it) {
    if (it->buf)
        buf_drop(it->buf);
    it->buf = nullptr;
}

static struct ext2_dir_entry* dir_iter_entry(struct dir_iter* it, size_t offset) {
    return (struct ext2_dir_entry*)((uint8_t*)it->buf->data + offset);
}

// Returns the next entry, or `nullptr` at the end. Deleted entries are returned too.
static errno_t dir_next(struct dir_iter* it, struct ext2_dir_entry** out) {
    struct ext2_super* sb = it->dir->sb;

    while (true) {
        if (!it->buf) {
            if (it->lblk >= it->num_blocks) {
                *out = nullptr;
                return 0;
            }

            errno_t status = ext2_bmap(it->dir, it->lblk, &it->block);
            if (status)
                return status;

            // Directories can't be sparse.
            if (!it->block) {
                log_warn("Directory inode %u has a hole at block %lu\n", it->dir->ino, it->lblk);
                return EUCLEAN;
            }

            status = buf_read(sb->dev, it->block, sb->block_size, &it->buf);
            if (status)
                return status;

            it->next = 0;
        }

        if (it->next >= sb->block_size) {
            dir_iter_finish(it);
            it->lblk++;
            continue;
        }

        // Entries must stay inside their block and point to a real inode.
        struct ext2_dir_entry* entry = dir_iter_entry(it, it->next);
        const size_t rec_len = le16_to_cpu(entry->rec_len);
        if (rec_len < DIRENT_HEADER || rec_len % 4 != 0 || it->next + rec_len > sb->block_size ||
            DIRENT_HEADER + entry->name_len > rec_len || le32_to_cpu(entry->inode) > sb->inodes_count) {
            log_warn("Directory inode %u has a corrupted entry in block %lu\n", it->dir->ino, it->lblk);
            return EUCLEAN;
        }

        it->offset = it->next;
        it->next += rec_len;

        *out = entry;
        return 0;
    }
}

// An inode number of 0 marks a deleted entry.
static bool entry_is(const struct ext2_dir_entry* entry, const char* name, size_t name_len) {
    return entry->inode != 0 && entry->name_len == name_len && !memcmp(entry->name, name, name_len);
}

static bool entry_is_dot(const struct ext2_dir_entry* entry) {
    return entry_is(entry, ".", 1) || entry_is(entry, "..", 2);
}

static void dir_iter_init(struct dir_iter* it, struct ext2_inode_info* dir) {
    *it = (struct dir_iter){
        .dir = dir,
        .num_blocks = (ext2_size(&dir->raw) + dir->sb->block_size - 1) / dir->sb->block_size,
    };
}

// On success, `it` still holds the block of the found entry.
static errno_t dir_find(struct ext2_inode_info* dir, const char* name, size_t name_len, struct dir_iter* it) {
    dir_iter_init(it, dir);

    while (true) {
        struct ext2_dir_entry* entry;
        errno_t status = dir_next(it, &entry);
        if (status) {
            dir_iter_finish(it);
            return status;
        }

        if (!entry)
            return ENOENT;
        if (entry_is(entry, name, name_len))
            return 0;
    }
}

static errno_t dir_lookup_ino(struct ext2_inode_info* dir, const char* name, size_t name_len, uint32_t* out) {
    struct dir_iter it;
    errno_t status = dir_find(dir, name, name_len, &it);
    if (status)
        return status;

    *out = le32_to_cpu(dir_iter_entry(&it, it.offset)->inode);
    dir_iter_finish(&it);
    return 0;
}

// TODO: Directory modification helpers.

static errno_t ext2_dir_lookup(struct vfs_node* node, struct vfs_path* child) {
    struct ext2_inode_info* dir = ext2_info(node);
    struct vfs_entry* entry = child->entry;

    // A removed directory has no entries left.
    rwlock_read_lock(&dir->lock);
    uint32_t ino = 0;
    errno_t status = dir->raw.i_links_count == 0 ? ENOENT : dir_lookup_ino(dir, entry->name, entry->name_len, &ino);
    rwlock_read_unlock(&dir->lock);

    if (status)
        return status;

    struct vfs_node* found;
    status = ext2_get_node(dir->sb, ino, &found);
    if (status)
        return status;

    vfs_entry_set_inode(entry, found);
    vfs_node_unref(found);
    return 0;
}

static uint8_t dirent_type(uint8_t file_type) {
    switch (file_type) {
    case EXT2_FT_REG_FILE:
        return __DT_REG;
    case EXT2_FT_DIR:
        return __DT_DIR;
    case EXT2_FT_CHRDEV:
        return __DT_CHR;
    case EXT2_FT_BLKDEV:
        return __DT_BLK;
    case EXT2_FT_FIFO:
        return __DT_FIFO;
    case EXT2_FT_SOCK:
        return __DT_SOCK;
    case EXT2_FT_SYMLINK:
        return __DT_LNK;
    default:
        return __DT_UNKNOWN;
    }
}

static errno_t ext2_dir_get_dir_entries(
    struct vfs_node* node,
    struct vfs_entry* entry,
    off_t offset,
    struct __dirent* buffer,
    size_t buffer_len,
    const struct ident* identity,
    size_t* out_count
) {
    struct ext2_inode_info* dir = ext2_info(node);
    size_t read = 0;

    // `offset` counts live entries, not bytes.
    off_t current = 0;

    rwlock_read_lock(&dir->lock);

    struct dir_iter it;
    dir_iter_init(&it, dir);

    errno_t status = 0;
    while (read < buffer_len) {
        struct ext2_dir_entry* raw;
        status = dir_next(&it, &raw);
        if (status || !raw)
            break;

        // The VFS adds `.` and `..` itself.
        if (raw->inode == 0 || entry_is_dot(raw))
            continue;
        if (current++ < offset)
            continue;

        struct __dirent* d = &buffer[read++];
        d->d_ino = le32_to_cpu(raw->inode);
        d->d_off = current;
        d->d_reclen = sizeof(*d);
        d->d_type = dirent_type(raw->file_type);
        memcpy(d->d_name, raw->name, raw->name_len);
        d->d_name[raw->name_len] = '\0';
    }

    dir_iter_finish(&it);
    rwlock_read_unlock(&dir->lock);

    *out_count = read;
    return status;
}

// TODO: create, mkdir, symlink, link, unlink, rmdir, rename, mknod.

static const struct vfs_file_ops ext2_dir_file_ops = {};

const struct vfs_node_ops ext2_dir_ops = {
    .destroy = ext2_node_destroy,
    .file_ops = &ext2_dir_file_ops,
    .dir = {
        .lookup = ext2_dir_lookup,
        .get_dir_entries = ext2_dir_get_dir_entries,
    },
};
