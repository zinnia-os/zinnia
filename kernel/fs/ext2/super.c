// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "ext2: " fmt

#include <fs/ext2/ext2.h>
#include <lib/clock.h>
#include <lib/endian.h>
#include <lib/module.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <string.h>

uint32_t ext2_now() {
    return clock_get_realtime_ns() / 1'000'000'000;
}

bool ext2_valid_block(struct ext2_super* sb, uint32_t block) {
    return block >= sb->first_data_block && block < sb->blocks_count;
}

static void group_desc_location(struct ext2_super* sb, uint32_t group, uint64_t* blkno, size_t* offset) {
    const uint64_t byte = (uint64_t)group * sizeof(struct ext2_group_desc);
    *blkno = sb->first_data_block + 1 + byte / sb->block_size;
    *offset = byte % sb->block_size;
}

// TODO: write_metadata and block/inode alloc.

static errno_t ext2_super_sync(struct vfs_super_block* base) {
    struct ext2_super* sb = ext2_sb(base);
    if (!sb->writable)
        return 0;

    // TODO: Write back inodes and file system metadata.
    return ENOSYS;
}

static errno_t ext2_super_statvfs(struct vfs_super_block* base, struct __statvfs* out) {
    struct ext2_super* sb = ext2_sb(base);

    mutex_lock(&sb->alloc_lock);
    *out = (struct __statvfs){
        .f_bsize = sb->block_size,
        .f_frsize = sb->block_size,
        .f_blocks = sb->blocks_count,
        .f_bfree = sb->free_blocks,
        .f_bavail = sb->free_blocks > sb->r_blocks_count ? sb->free_blocks - sb->r_blocks_count : 0,
        .f_files = sb->inodes_count,
        .f_ffree = sb->free_inodes,
        .f_favail = sb->free_inodes,
        .f_fsid = sb->dev->dev,
        .f_flag = ((base->flags & MNT_RDONLY) ? __ST_RDONLY : 0) | ((base->flags & MNT_NOSUID) ? __ST_NOSUID : 0),
        .f_namemax = EXT2_NAME_LEN,
    };
    mutex_unlock(&sb->alloc_lock);
    memcpy(out->f_basetype, "ext2", sizeof("ext2"));
    return 0;
}

static void ext2_super_destroy(struct vfs_super_block* base) {
    struct ext2_super* sb = ext2_sb(base);

    // TODO: Write back the clean state if writable.
    blockdev_release(sb->dev);
    blockdev_unref(sb->dev);
    mem_free(sb->groups, sb->group_count * sizeof(struct ext2_group));
    mem_free(sb, sizeof(*sb));
}

static const struct vfs_super_block_ops ext2_super_ops = {
    .sync = ext2_super_sync,
    .statvfs = ext2_super_statvfs,
    .destroy = ext2_super_destroy,
};

static errno_t ext2_mount(const struct vfs_filesystem* fs, enum mount_flags flags, void* arg, struct vfs_mount** out) {
    if (!arg)
        return EINVAL;

    // Open the block device at path `arg`.
    struct process* proc = percpu_get()->sched.current->process;

    mutex_lock(&proc->lock);
    struct vfs_path root = vfs_path_clone(proc->root);
    struct vfs_path cwd = vfs_path_clone(proc->cwd);
    mutex_unlock(&proc->lock);

    struct vfs_path target;
    errno_t status =
        vfs_path_lookup(root, cwd, arg, &proc->identity, VFS_LOOKUP_MUST_EXIST | VFS_LOOKUP_FOLLOW_SYMLINKS, &target);
    vfs_path_drop(root);
    vfs_path_drop(cwd);
    if (status)
        return status;

    struct vfs_node* node = vfs_entry_get_inode(target.entry);
    vfs_path_drop(target);
    if (!node)
        return ENOENT;

    struct blockdev* dev;
    status = blockdev_from_node(node, &dev);
    vfs_node_unref(node);
    if (status)
        return status;

    status = blockdev_claim(dev);
    if (status) {
        blockdev_unref(dev);
        return status;
    }

    struct ext2_super* sb = mem_alloc(sizeof(*sb), 0);
    if (!sb) {
        blockdev_release(dev);
        blockdev_unref(dev);
        return ENOMEM;
    }
    refcount_init(&sb->base.rc);
    sb->base.ops = &ext2_super_ops;
    sb->base.flags = flags;
    sb->dev = dev;
    mutex_init(&sb->alloc_lock);

    // Read and validate the super block.
    const size_t probe_size = MAX(sizeof(struct ext2_super_block), dev->lba_size);
    if (probe_size > page_size() || probe_size % dev->lba_size != 0) {
        status = EINVAL;
        goto out;
    }

    struct buf* buf;
    status = buf_read(dev, EXT2_SUPER_OFFSET / probe_size, probe_size, &buf);
    if (status)
        goto out;
    struct ext2_super_block disk;
    memcpy(&disk, (uint8_t*)buf->data + EXT2_SUPER_OFFSET % probe_size, sizeof(disk));
    buf_drop(buf);

    if (le16_to_cpu(disk.s_magic) != EXT2_MAGIC) {
        status = EINVAL;
        goto out;
    }

    const uint32_t log_block_size = le32_to_cpu(disk.s_log_block_size);
    if (log_block_size > 2) {
        log_err("Unsupported block size 2^%u KiB\n", log_block_size);
        status = EINVAL;
        goto out;
    }
    sb->block_size = EXT2_MIN_BLOCK_SIZE << log_block_size;
    if (sb->block_size > page_size() || sb->block_size % dev->lba_size != 0) {
        log_err("Block size %zu is incompatible with this device\n", sb->block_size);
        status = EINVAL;
        goto out;
    }

    // Read super block fields.
    const uint32_t rev_level = le32_to_cpu(disk.s_rev_level);
    sb->inode_size = rev_level >= 1 ? le16_to_cpu(disk.s_inode_size) : EXT2_GOOD_OLD_INODE_SIZE;
    sb->first_ino = rev_level >= 1 ? le32_to_cpu(disk.s_first_ino) : EXT2_GOOD_OLD_FIRST_INO;
    sb->feature_incompat = rev_level >= 1 ? le32_to_cpu(disk.s_feature_incompat) : 0;
    sb->feature_ro_compat = rev_level >= 1 ? le32_to_cpu(disk.s_feature_ro_compat) : 0;

    sb->blocks_count = le32_to_cpu(disk.s_blocks_count);
    sb->inodes_count = le32_to_cpu(disk.s_inodes_count);
    sb->r_blocks_count = le32_to_cpu(disk.s_r_blocks_count);
    sb->free_blocks = le32_to_cpu(disk.s_free_blocks_count);
    sb->free_inodes = le32_to_cpu(disk.s_free_inodes_count);
    sb->first_data_block = le32_to_cpu(disk.s_first_data_block);
    sb->blocks_per_group = le32_to_cpu(disk.s_blocks_per_group);
    sb->inodes_per_group = le32_to_cpu(disk.s_inodes_per_group);
    sb->state = le16_to_cpu(disk.s_state);
    sb->mnt_count = le16_to_cpu(disk.s_mnt_count);

    // Quick sanity check.
    if (sb->inode_size < EXT2_GOOD_OLD_INODE_SIZE || (sb->inode_size & (sb->inode_size - 1)) != 0 ||
        sb->inode_size > sb->block_size || sb->blocks_per_group == 0 || sb->blocks_per_group > sb->block_size * 8 ||
        sb->inodes_per_group == 0 || sb->inodes_per_group > sb->block_size * 8 ||
        sb->first_data_block >= sb->blocks_count || sb->first_ino < EXT2_GOOD_OLD_FIRST_INO ||
        sb->first_ino > sb->inodes_count ||
        (uint64_t)sb->blocks_count * (sb->block_size / dev->lba_size) > dev->lba_count) {
        log_err("Super block is corrupted\n");
        status = EINVAL;
        goto out;
    }

    sb->group_count = (sb->blocks_count - sb->first_data_block + sb->blocks_per_group - 1) / sb->blocks_per_group;
    if ((uint64_t)sb->group_count * sb->inodes_per_group < sb->inodes_count) {
        log_err("Inode count exceeds the group layout\n");
        status = EINVAL;
        goto out;
    }

    const uint32_t unsup_incompat = sb->feature_incompat & ~EXT2_SUPPORTED_INCOMPAT;
    if (unsup_incompat) {
        log_err("Unsupported incompatible features %#x\n", unsup_incompat);
        status = EINVAL;
        goto out;
    }

    const uint32_t unsup_ro_compat = sb->feature_ro_compat & ~EXT2_SUPPORTED_RO_COMPAT;
    if (unsup_ro_compat && !(flags & MNT_RDONLY)) {
        log_err("Unsupported read-only compatible features %#x, mount read-only\n", unsup_ro_compat);
        status = EROFS;
        goto out;
    }

    // Limit the file size by the block map reach and the sector count.
    // TODO: This code could probably be more compressed, but that makes it harder to understand what is going on.
    const uint64_t ptrs_per_block = sb->block_size / sizeof(uint32_t);
    const uint64_t ind_data = ptrs_per_block;
    const uint64_t dind_data = ptrs_per_block * ind_data;
    const uint64_t tind_data = ptrs_per_block * dind_data;
    const uint64_t mappable_blocks = EXT2_NDIR_BLOCKS + ind_data + dind_data + tind_data;

    const uint64_t ind_meta = 1;
    const uint64_t dind_meta = 1 + ptrs_per_block * ind_meta;
    const uint64_t tind_meta = 1 + ptrs_per_block * dind_meta;
    const uint64_t indirect_blocks = ind_meta + dind_meta + tind_meta;

    const uint64_t sectors_per_block = sb->block_size / EXT2_SECTOR_SIZE;
    const uint64_t countable_blocks = UINT32_MAX / sectors_per_block;
    const uint64_t countable_data = countable_blocks - indirect_blocks;

    sb->max_file_size = MIN(mappable_blocks, countable_data) * sb->block_size;
    if (!(sb->feature_ro_compat & EXT2_FEATURE_RO_COMPAT_LARGE_FILE))
        sb->max_file_size = MIN(sb->max_file_size, EXT2_LARGE_FILE_THRESHOLD - 1);

    // Read and validate the group descriptors.
    sb->groups = mem_alloc(sb->group_count * sizeof(struct ext2_group), 0);
    if (!sb->groups) {
        status = ENOMEM;
        goto out;
    }

    for (uint32_t i = 0; i < sb->group_count; i++) {
        uint64_t blkno;
        size_t offset;
        group_desc_location(sb, i, &blkno, &offset);

        status = buf_read(sb->dev, blkno, sb->block_size, &buf);
        if (status)
            goto out;
        const struct ext2_group_desc* desc = (struct ext2_group_desc*)((uint8_t*)buf->data + offset);
        sb->groups[i] = (struct ext2_group){
            .block_bitmap = le32_to_cpu(desc->bg_block_bitmap),
            .inode_bitmap = le32_to_cpu(desc->bg_inode_bitmap),
            .inode_table = le32_to_cpu(desc->bg_inode_table),
            .free_blocks = le16_to_cpu(desc->bg_free_blocks_count),
            .free_inodes = le16_to_cpu(desc->bg_free_inodes_count),
            .used_dirs = le16_to_cpu(desc->bg_used_dirs_count),
        };
        buf_drop(buf);

        const uint64_t table_blocks =
            ((uint64_t)sb->inodes_per_group * sb->inode_size + sb->block_size - 1) / sb->block_size;
        const struct ext2_group* group = &sb->groups[i];
        if (!ext2_valid_block(sb, group->block_bitmap) || !ext2_valid_block(sb, group->inode_bitmap) ||
            !ext2_valid_block(sb, group->inode_table) || group->inode_table + table_blocks > sb->blocks_count) {
            log_err("Group descriptor %u is corrupted\n", i);
            status = EINVAL;
            goto out;
        }
    }

    // TODO: Mark the file system as mounted.

    // TODO: Load the root inode and create the mount.
    status = ENOSYS;

out:
    vfs_super_block_unref(&sb->base);
    return status;
}

static const struct vfs_filesystem ext2_fs = {
    .name = "ext2",
    .mount = ext2_mount,
};

static errno_t ext2_init(struct module* module) {
    return vfs_register_filesystem(&ext2_fs);
}

MODULE_ENTRY(ext2_init);
MODULE_DESC("ext2 file system");
MODULE_AUTHOR("Marvin Friedrich");
MODULE_LICENSE("BSD-3-Clause");
MODULE_VERSION("1.0");
