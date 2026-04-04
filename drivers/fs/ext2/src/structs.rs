//! Ext2 on-disk data structures.

pub const EXT2_MAGIC: u16 = 0xEF53;

pub const EXT2_ROOT_INO: u32 = 2;

pub const EXT2_NDIR_BLOCKS: usize = 12;

pub const EXT2_IND_BLOCK: usize = 12;

pub const EXT2_DIND_BLOCK: usize = 13;

pub const EXT2_TIND_BLOCK: usize = 14;

pub const EXT2_N_BLOCKS: usize = 15;

pub const EXT2_FT_UNKNOWN: u8 = 0;
pub const EXT2_FT_REG_FILE: u8 = 1;
pub const EXT2_FT_DIR: u8 = 2;
pub const EXT2_FT_CHRDEV: u8 = 3;
pub const EXT2_FT_BLKDEV: u8 = 4;
pub const EXT2_FT_FIFO: u8 = 5;
pub const EXT2_FT_SOCK: u8 = 6;
pub const EXT2_FT_SYMLINK: u8 = 7;

pub const EXT2_FEATURE_COMPAT_DIR_PREALLOC: u32 = 0x0001;
pub const EXT2_FEATURE_COMPAT_RESIZE_INO: u32 = 0x0010;
pub const EXT2_FEATURE_COMPAT_DIR_INDEX: u32 = 0x0020;

pub const EXT2_FEATURE_INCOMPAT_FILETYPE: u32 = 0x0002;

pub const EXT2_SUPPORTED_INCOMPAT: u32 = EXT2_FEATURE_INCOMPAT_FILETYPE;

pub const EXT2_FEATURE_RO_COMPAT_SPARSE_SUPER: u32 = 0x0001;
pub const EXT2_FEATURE_RO_COMPAT_LARGE_FILE: u32 = 0x0002;

pub const EXT2_SUPPORTED_RO_COMPAT: u32 =
    EXT2_FEATURE_RO_COMPAT_SPARSE_SUPER | EXT2_FEATURE_RO_COMPAT_LARGE_FILE;

pub const S_IFMT: u16 = 0xF000;
pub const S_IFSOCK: u16 = 0xC000;
pub const S_IFLNK: u16 = 0xA000;
pub const S_IFREG: u16 = 0x8000;
pub const S_IFBLK: u16 = 0x6000;
pub const S_IFDIR: u16 = 0x4000;
pub const S_IFCHR: u16 = 0x2000;
pub const S_IFIFO: u16 = 0x1000;

/// The ext2 superblock, located at byte offset 1024 on disk.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Ext2SuperBlock {
    pub s_inodes_count: u32,
    pub s_blocks_count: u32,
    pub s_r_blocks_count: u32,
    pub s_free_blocks_count: u32,
    pub s_free_inodes_count: u32,
    pub s_first_data_block: u32,
    pub s_log_block_size: u32,
    pub s_log_frag_size: u32,
    pub s_blocks_per_group: u32,
    pub s_frags_per_group: u32,
    pub s_inodes_per_group: u32,
    pub s_mtime: u32,
    pub s_wtime: u32,
    pub s_mnt_count: u16,
    pub s_max_mnt_count: u16,
    pub s_magic: u16,
    pub s_state: u16,
    pub s_errors: u16,
    pub s_minor_rev_level: u16,
    pub s_lastcheck: u32,
    pub s_checkinterval: u32,
    pub s_creator_os: u32,
    pub s_rev_level: u32,
    pub s_def_resuid: u16,
    pub s_def_resgid: u16,
    // Extended superblock fields (rev >= 1)
    pub s_first_ino: u32,
    pub s_inode_size: u16,
    pub s_block_group_nr: u16,
    pub s_feature_compat: u32,
    pub s_feature_incompat: u32,
    pub s_feature_ro_compat: u32,
    pub s_uuid: [u8; 16],
    pub s_volume_name: [u8; 16],
    pub s_last_mounted: [u8; 64],
    pub s_algo_bitmap: u32,
    // Padding to 1024 bytes total (we only care about the fields above).
    pub _padding: [u8; 820 - 204],
}

/// Block group descriptor.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Ext2BlockGroupDesc {
    pub bg_block_bitmap: u32,
    pub bg_inode_bitmap: u32,
    pub bg_inode_table: u32,
    pub bg_free_blocks_count: u16,
    pub bg_free_inodes_count: u16,
    pub bg_used_dirs_count: u16,
    pub bg_pad: u16,
    pub bg_reserved: [u8; 12],
}

/// On-disk inode structure.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Ext2Inode {
    pub i_mode: u16,
    pub i_uid: u16,
    pub i_size: u32,
    pub i_atime: u32,
    pub i_ctime: u32,
    pub i_mtime: u32,
    pub i_dtime: u32,
    pub i_gid: u16,
    pub i_links_count: u16,
    pub i_blocks: u32,
    pub i_flags: u32,
    pub i_osd1: u32,
    pub i_block: [u32; EXT2_N_BLOCKS],
    pub i_generation: u32,
    pub i_file_acl: u32,
    pub i_dir_acl: u32,
    pub i_faddr: u32,
    pub i_osd2: [u8; 12],
}

/// On-disk directory entry.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Ext2DirEntry {
    pub inode: u32,
    pub rec_len: u16,
    pub name_len: u8,
    pub file_type: u8,
    // name bytes follow (up to 255 bytes), but we read them separately.
}

impl Ext2SuperBlock {
    /// Returns the block size in bytes: 1024 << s_log_block_size.
    pub fn block_size(&self) -> usize {
        1024 << self.s_log_block_size
    }

    /// Returns the number of block groups.
    pub fn block_group_count(&self) -> u32 {
        self.s_blocks_count.div_ceil(self.s_blocks_per_group)
    }

    /// Returns the inode size (for rev 0, always 128).
    pub fn inode_size(&self) -> usize {
        if self.s_rev_level >= 1 {
            self.s_inode_size as usize
        } else {
            128
        }
    }
}

impl Ext2Inode {
    /// Returns the full 64-bit file size (for regular files, uses i_dir_acl as high 32 bits).
    pub fn size(&self) -> u64 {
        let lo = self.i_size as u64;
        let hi = if self.i_mode & S_IFMT == S_IFREG {
            (self.i_dir_acl as u64) << 32
        } else {
            0
        };
        lo | hi
    }

    /// Returns true if this is a directory.
    pub fn is_dir(&self) -> bool {
        self.i_mode & S_IFMT == S_IFDIR
    }

    /// Returns true if this is a regular file.
    pub fn is_regular(&self) -> bool {
        self.i_mode & S_IFMT == S_IFREG
    }

    /// Returns true if this is a symlink.
    pub fn is_symlink(&self) -> bool {
        self.i_mode & S_IFMT == S_IFLNK
    }

    /// For fast symlinks, returns the inline target data (stored in i_block).
    /// Fast symlinks are those with size <= 60 bytes and i_blocks == 0.
    pub fn is_fast_symlink(&self) -> bool {
        self.is_symlink() && self.i_blocks == 0 && self.size() <= 60
    }

    /// Returns the inline symlink target bytes (from i_block array).
    pub fn fast_symlink_target(&self) -> &[u8] {
        let ptr = self.i_block.as_ptr() as *const u8;
        let len = self.size() as usize;
        unsafe { core::slice::from_raw_parts(ptr, len.min(60)) }
    }
}
