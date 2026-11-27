// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <dev/block/blockdev.h>
#include <dev/block/buf.h>
#include <fs/vfs.h>
#include <lib/condvar.h>
#include <lib/mutex.h>
#include <lib/radix.h>
#include <lib/rwlock.h>
#include <lib/utils.h>
#include <mem/vmo.h>
#include <stddef.h>
#include <stdint.h>

constexpr uint16_t EXT2_MAGIC = 0xEF53;
constexpr uint32_t EXT2_ROOT_INO = 2;
constexpr uint32_t EXT2_GOOD_OLD_FIRST_INO = 11;
constexpr size_t EXT2_GOOD_OLD_INODE_SIZE = 128;
constexpr size_t EXT2_SUPER_OFFSET = 1024;
constexpr size_t EXT2_MIN_BLOCK_SIZE = 1024;
constexpr size_t EXT2_MAX_BLOCK_SIZE = 4096;
constexpr size_t EXT2_SECTOR_SIZE = 512;

constexpr size_t EXT2_NDIR_BLOCKS = 12;
constexpr size_t EXT2_IND_BLOCK = 12;
constexpr size_t EXT2_DIND_BLOCK = 13;
constexpr size_t EXT2_TIND_BLOCK = 14;
constexpr size_t EXT2_N_BLOCKS = 15;
constexpr size_t EXT2_FAST_SYMLINK_MAX = EXT2_N_BLOCKS * sizeof(uint32_t);

constexpr uint16_t EXT2_VALID_FS = 1 << 0;
constexpr uint32_t EXT2_INDEX_FL = 0x1000;
constexpr uint16_t EXT2_LINK_MAX = 32000;
constexpr size_t EXT2_NAME_LEN = 255;
constexpr uint64_t EXT2_LARGE_FILE_THRESHOLD = 1ull << 31;

constexpr uint32_t EXT2_FEATURE_INCOMPAT_FILETYPE = 0x0002;
constexpr uint32_t EXT2_FEATURE_RO_COMPAT_SPARSE_SUPER = 0x0001;
constexpr uint32_t EXT2_FEATURE_RO_COMPAT_LARGE_FILE = 0x0002;
constexpr uint32_t EXT2_SUPPORTED_INCOMPAT = EXT2_FEATURE_INCOMPAT_FILETYPE;
constexpr uint32_t EXT2_SUPPORTED_RO_COMPAT = EXT2_FEATURE_RO_COMPAT_SPARSE_SUPER | EXT2_FEATURE_RO_COMPAT_LARGE_FILE;

enum ext2_file_type : uint8_t {
    EXT2_FT_UNKNOWN = 0,
    EXT2_FT_REG_FILE = 1,
    EXT2_FT_DIR = 2,
    EXT2_FT_CHRDEV = 3,
    EXT2_FT_BLKDEV = 4,
    EXT2_FT_FIFO = 5,
    EXT2_FT_SOCK = 6,
    EXT2_FT_SYMLINK = 7,
};

struct ext2_super_block {
    uint32_t s_inodes_count;
    uint32_t s_blocks_count;
    uint32_t s_r_blocks_count;
    uint32_t s_free_blocks_count;
    uint32_t s_free_inodes_count;
    uint32_t s_first_data_block;
    uint32_t s_log_block_size;
    uint32_t s_log_frag_size;
    uint32_t s_blocks_per_group;
    uint32_t s_frags_per_group;
    uint32_t s_inodes_per_group;
    uint32_t s_mtime;
    uint32_t s_wtime;
    uint16_t s_mnt_count;
    uint16_t s_max_mnt_count;
    uint16_t s_magic;
    uint16_t s_state;
    uint16_t s_errors;
    uint16_t s_minor_rev_level;
    uint32_t s_lastcheck;
    uint32_t s_checkinterval;
    uint32_t s_creator_os;
    uint32_t s_rev_level;
    uint16_t s_def_resuid;
    uint16_t s_def_resgid;
    uint32_t s_first_ino;
    uint16_t s_inode_size;
    uint16_t s_block_group_nr;
    uint32_t s_feature_compat;
    uint32_t s_feature_incompat;
    uint32_t s_feature_ro_compat;
    uint8_t s_uuid[16];
    char s_volume_name[16];
    char s_last_mounted[64];
    uint32_t s_algo_bitmap;
    uint8_t s_reserved[820];
};
static_assert(sizeof(struct ext2_super_block) == 1024);

struct ext2_group_desc {
    uint32_t bg_block_bitmap;
    uint32_t bg_inode_bitmap;
    uint32_t bg_inode_table;
    uint16_t bg_free_blocks_count;
    uint16_t bg_free_inodes_count;
    uint16_t bg_used_dirs_count;
    uint16_t bg_pad;
    uint8_t bg_reserved[12];
};
static_assert(sizeof(struct ext2_group_desc) == 32);

struct ext2_inode {
    uint16_t i_mode;
    uint16_t i_uid;
    uint32_t i_size;
    uint32_t i_atime;
    uint32_t i_ctime;
    uint32_t i_mtime;
    uint32_t i_dtime;
    uint16_t i_gid;
    uint16_t i_links_count;
    uint32_t i_blocks;
    uint32_t i_flags;
    uint32_t i_osd1;
    uint32_t i_block[EXT2_N_BLOCKS];
    uint32_t i_generation;
    uint32_t i_file_acl;
    uint32_t i_size_high;
    uint32_t i_faddr;
    uint8_t i_frag;
    uint8_t i_fsize;
    uint16_t i_pad1;
    uint16_t i_uid_high;
    uint16_t i_gid_high;
    uint32_t i_reserved2;
};
static_assert(sizeof(struct ext2_inode) == EXT2_GOOD_OLD_INODE_SIZE);

struct ext2_dir_entry {
    uint32_t inode;
    uint16_t rec_len;
    uint8_t name_len;
    uint8_t file_type;
    char name[];
};
static_assert(sizeof(struct ext2_dir_entry) == 8);

struct ext2_group {
    uint32_t block_bitmap;
    uint32_t inode_bitmap;
    uint32_t inode_table;
    uint32_t free_blocks;
    uint32_t free_inodes;
    uint32_t used_dirs;
};

struct ext2_super {
    struct vfs_super_block base;
    struct blockdev* dev;
    bool writable;

    size_t block_size;
    size_t inode_size;
    uint32_t blocks_count;
    uint32_t inodes_count;
    uint32_t r_blocks_count;
    uint32_t first_data_block;
    uint32_t blocks_per_group;
    uint32_t inodes_per_group;
    uint32_t first_ino;
    uint32_t group_count;
    uint32_t feature_incompat;
    uint64_t max_file_size;

    mutex_t alloc_lock;
    uint32_t feature_ro_compat;
    uint32_t free_blocks;
    uint32_t free_inodes;
    uint16_t state;
    uint16_t mnt_count;
    struct ext2_group* groups;

    mutex_t icache_lock;
    struct condvar icache_cv;
    struct radix_tree icache;
};

struct ext2_inode_info {
    struct vmo vmo;
    struct ext2_super* sb;
    uint32_t ino;
    struct rwlock lock;
    struct ext2_inode raw;
    struct vfs_node* vnode;
};

static inline struct ext2_super* ext2_sb(struct vfs_super_block* sb) {
    return containerof(sb, struct ext2_super, base);
}

static inline struct ext2_inode_info* ext2_info(struct vfs_node* node) {
    return node->priv;
}

static inline uint32_t ext2_group_of_ino(struct ext2_super* sb, uint32_t ino) {
    return (ino - 1) / sb->inodes_per_group;
}

static inline bool ext2_is_dir(const struct ext2_inode* raw) {
    return (raw->i_mode & __S_IFMT) == __S_IFDIR;
}

static inline uid_t ext2_uid(const struct ext2_inode* raw) {
    return raw->i_uid | ((uid_t)raw->i_uid_high << 16);
}

static inline gid_t ext2_gid(const struct ext2_inode* raw) {
    return raw->i_gid | ((gid_t)raw->i_gid_high << 16);
}

static inline void ext2_set_owner(struct ext2_inode* raw, uid_t uid, gid_t gid) {
    raw->i_uid = uid & 0xFFFF;
    raw->i_uid_high = uid >> 16;
    raw->i_gid = gid & 0xFFFF;
    raw->i_gid_high = gid >> 16;
}

static inline uint64_t ext2_size(const struct ext2_inode* raw) {
    uint64_t size = raw->i_size;
    if ((raw->i_mode & __S_IFMT) == __S_IFREG)
        size |= (uint64_t)raw->i_size_high << 32;
    return size;
}

// Current timestamp in ext2 units.
uint32_t ext2_now();

bool ext2_valid_block(struct ext2_super* sb, uint32_t block);

// TODO: Add block/inode allocation.

errno_t ext2_read_inode(struct ext2_super* sb, uint32_t ino, struct ext2_inode* out);
void ext2_update_vnode(struct vfs_node* node, struct ext2_inode_info* info);
bool ext2_has_blocks(struct ext2_inode_info* info);

errno_t ext2_bmap(struct ext2_inode_info* info, uint64_t lblk, uint32_t* out);

errno_t ext2_get_node(struct ext2_super* sb, uint32_t ino, struct vfs_node** out);
void ext2_node_destroy(struct vfs_node* node);

extern const struct vfs_node_ops ext2_dir_ops;
extern const struct vfs_node_ops ext2_regular_ops;
extern const struct vfs_node_ops ext2_symlink_ops;
extern const struct vfs_node_ops ext2_special_ops;
