#![no_std]

pub mod inode;
pub mod structs;

use inode::{Ext2Dir, Ext2Regular, Ext2Symlink};
use structs::*;

use zinnia::{
    alloc::{collections::btree_map::BTreeMap, sync::Arc, vec::Vec},
    core::slice,
    device::block::BlockDevice,
    error, log,
    memory::{AllocFlags, KernelAlloc, PageAllocator, PhysAddr, UserCStr, UserPtr},
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    uapi::{limits::PATH_MAX, statvfs::statvfs, time::timespec},
    util::mutex::spin::SpinMutex,
    vfs::{
        cache::{Entry, LookupFlags, PathNode},
        fs::{FileSystem, Mount, MountFlags, SuperBlock},
        inode::{INode, Mode, NodeOps},
    },
};

/// The ext2 filesystem driver.
#[derive(Debug)]
pub struct Ext2Fs;

impl FileSystem for Ext2Fs {
    fn get_name(&self) -> &[u8] {
        b"ext2"
    }

    fn mount(&self, flags: MountFlags, arg: UserPtr<()>) -> EResult<Arc<Mount>> {
        // For ext2, data is the source device path (e.g. "/dev/nvme0n1p1").
        let data = UserCStr::new(arg.addr())
            .as_vec(PATH_MAX)
            .ok_or(Errno::EFAULT)?;

        let task = Scheduler::get_current();
        let proc = task.get_process();

        let path = PathNode::lookup(
            proc.root_dir.lock().clone(),
            proc.working_dir.lock().clone(),
            &data,
            &proc.identity.lock(),
            LookupFlags::MustExist | LookupFlags::FollowSymlinks,
        )?;
        let inode = path.entry.get_inode().ok_or(Errno::ENOENT)?;
        let dev = match &inode.node_ops {
            NodeOps::BlockDevice(file_ops) => file_ops,
            _ => return Err(Errno::EINVAL)?,
        };

        let sb = Ext2Super::read(dev.clone())?;
        let sb = Arc::new(sb);

        // Load the root inode.
        let root_inode = sb.clone().read_inode(EXT2_ROOT_INO)?;
        let root_vfs_inode = sb.clone().inode_to_vfs(EXT2_ROOT_INO, &root_inode)?;

        let root_entry = Arc::new(Entry::new(b"", Some(root_vfs_inode), None));

        Ok(Arc::new(Mount::new(flags, root_entry)))
    }
}

/// The in-memory ext2 superblock.
pub struct Ext2Super {
    pub device: Arc<dyn BlockDevice>,
    pub raw: SpinMutex<Ext2SuperBlock>,
    pub block_size: usize,
    pub inodes_per_group: u32,
    pub blocks_per_group: u32,
    pub inode_size: usize,
    pub group_count: u32,
    pub bgdt: SpinMutex<Vec<Ext2BlockGroupDesc>>,
    /// Cache of loaded VFS inodes, keyed by inode number.
    pub inode_cache: SpinMutex<BTreeMap<u32, Arc<INode>>>,
}

impl core::fmt::Debug for Ext2Super {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Ext2Super")
            .field("block_size", &self.block_size)
            .field("group_count", &self.group_count)
            .finish()
    }
}

impl Ext2Super {
    /// Read and validate the ext2 superblock from a block device.
    pub fn read(device: Arc<dyn BlockDevice>) -> EResult<Self> {
        let lba_size = device.get_lba_size();
        if lba_size == 0 {
            return Err(Errno::EINVAL);
        }

        // Superblock is at byte offset 1024, size 1024 bytes.
        // We need to read at least 2048 bytes from the start to cover it.
        let read_size = 2048usize.max(lba_size);
        let read_lbas = read_size.div_ceil(lba_size);
        let buf_size = read_lbas * lba_size;

        let buf = KernelAlloc::alloc_bytes(buf_size, AllocFlags::empty())?;
        let result = (|| -> EResult<Self> {
            device.read_lba(buf, read_lbas, 0)?;

            let bytes: &[u8] = unsafe { slice::from_raw_parts(buf.as_hhdm(), buf_size) };

            if bytes.len() < 1024 + size_of::<Ext2SuperBlock>() {
                return Err(Errno::EINVAL);
            }

            let raw: Ext2SuperBlock = unsafe {
                core::ptr::read_unaligned(bytes.as_ptr().add(1024) as *const Ext2SuperBlock)
            };

            if raw.s_magic != EXT2_MAGIC {
                error!("ext2: bad magic: {:#x}", raw.s_magic);
                return Err(Errno::EINVAL);
            }

            let block_size = raw.block_size();
            let group_count = raw.block_group_count();
            let inode_size = raw.inode_size();

            log!(
                "ext2: block_size={}, inodes={}, blocks={}, groups={}, inode_size={}",
                block_size,
                raw.s_inodes_count,
                raw.s_blocks_count,
                group_count,
                inode_size,
            );

            // Check feature flags — refuse to mount if unsupported features are present.
            let unsupported_incompat = raw.s_feature_incompat & !EXT2_SUPPORTED_INCOMPAT;
            if unsupported_incompat != 0 {
                error!(
                    "ext2: filesystem has unsupported INCOMPAT features: {:#x}",
                    unsupported_incompat
                );
                return Err(Errno::EINVAL);
            }

            let unsupported_ro_compat = raw.s_feature_ro_compat & !EXT2_SUPPORTED_RO_COMPAT;
            if unsupported_ro_compat != 0 {
                error!(
                    "ext2: filesystem has unsupported RO_COMPAT features: {:#x} (would need read-only mount)",
                    unsupported_ro_compat
                );
                return Err(Errno::EINVAL);
            }

            // Read the block group descriptor table.
            // It's located at block (s_first_data_block + 1).
            let bgdt_block = raw.s_first_data_block as u64 + 1;
            let bgdt_bytes = group_count as usize * size_of::<Ext2BlockGroupDesc>();
            let bgdt_lba_start = (bgdt_block * block_size as u64) / lba_size as u64;
            let bgdt_lba_count = bgdt_bytes.div_ceil(lba_size);
            let bgdt_buf_size = bgdt_lba_count * lba_size;

            let bgdt_buf = KernelAlloc::alloc_bytes(bgdt_buf_size, AllocFlags::empty())?;
            let bgdt_result = (|| -> EResult<Vec<Ext2BlockGroupDesc>> {
                device.read_lba(bgdt_buf, bgdt_lba_count, bgdt_lba_start)?;

                let bgdt_bytes: &[u8] =
                    unsafe { slice::from_raw_parts(bgdt_buf.as_hhdm(), bgdt_buf_size) };

                let mut groups = Vec::new();
                for i in 0..group_count as usize {
                    let off = i * size_of::<Ext2BlockGroupDesc>();
                    let desc: Ext2BlockGroupDesc = unsafe {
                        core::ptr::read_unaligned(
                            bgdt_bytes.as_ptr().add(off) as *const Ext2BlockGroupDesc
                        )
                    };
                    groups.push(desc);
                }
                Ok(groups)
            })();

            unsafe { KernelAlloc::dealloc_bytes(bgdt_buf, bgdt_buf_size) };
            let bgdt = bgdt_result?;

            Ok(Self {
                device,
                block_size,
                inodes_per_group: raw.s_inodes_per_group,
                blocks_per_group: raw.s_blocks_per_group,
                inode_size,
                group_count,
                bgdt: SpinMutex::new(bgdt),
                inode_cache: SpinMutex::new(BTreeMap::new()),
                raw: SpinMutex::new(raw),
            })
        })();

        unsafe { KernelAlloc::dealloc_bytes(buf, buf_size) };
        result
    }

    /// Get or create a cached VFS inode.
    pub fn get_or_load_inode(self: &Arc<Self>, ino: u32) -> EResult<Arc<INode>> {
        // Check cache first.
        if let Some(cached) = self.inode_cache.lock().get(&ino) {
            return Ok(cached.clone());
        }

        let raw = self.read_inode(ino)?;
        let vfs_inode = self.clone().inode_to_vfs(ino, &raw)?;

        self.inode_cache.lock().insert(ino, vfs_inode.clone());
        Ok(vfs_inode)
    }

    /// Read a raw ext2 inode from disk.
    pub fn read_inode(&self, ino: u32) -> EResult<Ext2Inode> {
        if ino == 0 {
            return Err(Errno::EINVAL);
        }

        let group = ((ino - 1) / self.inodes_per_group) as usize;
        let index = ((ino - 1) % self.inodes_per_group) as usize;

        let bgdt = self.bgdt.lock();
        if group >= bgdt.len() {
            return Err(Errno::EINVAL);
        }

        let inode_table_block = bgdt[group].bg_inode_table as u64;
        let byte_offset =
            inode_table_block * self.block_size as u64 + (index * self.inode_size) as u64;

        let lba_size = self.device.get_lba_size();
        let lba = byte_offset / lba_size as u64;
        let lba_offset = (byte_offset % lba_size as u64) as usize;

        // Read enough LBAs to cover the inode.
        let read_bytes = lba_offset + self.inode_size;
        let num_lbas = read_bytes.div_ceil(lba_size);
        let buf_size = num_lbas * lba_size;

        let buf = KernelAlloc::alloc_bytes(buf_size, AllocFlags::empty())?;
        let result = (|| -> EResult<Ext2Inode> {
            self.device.read_lba(buf, num_lbas, lba)?;
            let bytes: &[u8] = unsafe { slice::from_raw_parts(buf.as_hhdm(), buf_size) };
            let raw: Ext2Inode = unsafe {
                core::ptr::read_unaligned(bytes.as_ptr().add(lba_offset) as *const Ext2Inode)
            };
            Ok(raw)
        })();

        unsafe { KernelAlloc::dealloc_bytes(buf, buf_size) };
        result
    }

    /// Write a raw ext2 inode back to disk.
    pub fn write_inode(&self, ino: u32, raw: &Ext2Inode) -> EResult<()> {
        if ino == 0 {
            return Err(Errno::EINVAL);
        }

        let group = ((ino - 1) / self.inodes_per_group) as usize;
        let index = ((ino - 1) % self.inodes_per_group) as usize;

        let bgdt = self.bgdt.lock();
        if group >= bgdt.len() {
            return Err(Errno::EINVAL);
        }

        let inode_table_block = bgdt[group].bg_inode_table as u64;
        let byte_offset =
            inode_table_block * self.block_size as u64 + (index * self.inode_size) as u64;

        let lba_size = self.device.get_lba_size();
        let lba = byte_offset / lba_size as u64;
        let lba_offset = (byte_offset % lba_size as u64) as usize;

        // Read-modify-write the LBA(s) containing this inode.
        let read_bytes = lba_offset + self.inode_size;
        let num_lbas = read_bytes.div_ceil(lba_size);
        let buf_size = num_lbas * lba_size;

        let buf = KernelAlloc::alloc_bytes(buf_size, AllocFlags::empty())?;
        let result = (|| -> EResult<()> {
            self.device.read_lba(buf, num_lbas, lba)?;
            let bytes: &mut [u8] = unsafe { slice::from_raw_parts_mut(buf.as_hhdm(), buf_size) };

            let inode_bytes = unsafe {
                slice::from_raw_parts(raw as *const Ext2Inode as *const u8, size_of::<Ext2Inode>())
            };
            bytes[lba_offset..lba_offset + size_of::<Ext2Inode>()].copy_from_slice(inode_bytes);

            // Write back all affected LBAs.
            for i in 0..num_lbas {
                let write_buf = PhysAddr::new(buf.value() + i * lba_size);
                self.device.write_lba(write_buf, lba + i as u64)?;
            }
            Ok(())
        })();

        unsafe { KernelAlloc::dealloc_bytes(buf, buf_size) };
        result
    }

    /// Read a block from the filesystem into a caller-provided buffer.
    pub fn read_block(&self, block: u64, buf: &mut [u8]) -> EResult<()> {
        let byte_offset = block * self.block_size as u64;
        let lba_size = self.device.get_lba_size();
        let start_lba = byte_offset / lba_size as u64;
        let num_lbas = self.block_size.div_ceil(lba_size);

        let phys = KernelAlloc::alloc_bytes(num_lbas * lba_size, AllocFlags::empty())?;
        let result = (|| -> EResult<()> {
            self.device.read_lba(phys, num_lbas, start_lba)?;
            let data: &[u8] = unsafe { slice::from_raw_parts(phys.as_hhdm(), num_lbas * lba_size) };
            let copy_len = buf.len().min(self.block_size);
            buf[..copy_len].copy_from_slice(&data[..copy_len]);
            Ok(())
        })();

        unsafe { KernelAlloc::dealloc_bytes(phys, num_lbas * lba_size) };
        result
    }

    /// Write a block to the filesystem from a caller-provided buffer.
    pub fn write_block(&self, block: u64, buf: &[u8]) -> EResult<()> {
        let byte_offset = block * self.block_size as u64;
        let lba_size = self.device.get_lba_size();
        let start_lba = byte_offset / lba_size as u64;
        let num_lbas = self.block_size.div_ceil(lba_size);

        let phys = KernelAlloc::alloc_bytes(num_lbas * lba_size, AllocFlags::empty())?;
        let result = (|| -> EResult<()> {
            let data: &mut [u8] =
                unsafe { slice::from_raw_parts_mut(phys.as_hhdm(), num_lbas * lba_size) };
            let copy_len = buf.len().min(self.block_size);
            // Zero the buffer first in case block_size < lba_size * num_lbas.
            data.fill(0);
            data[..copy_len].copy_from_slice(&buf[..copy_len]);

            for i in 0..num_lbas {
                let write_buf = PhysAddr::new(phys.value() + i * lba_size);
                self.device.write_lba(write_buf, start_lba + i as u64)?;
            }
            Ok(())
        })();

        unsafe { KernelAlloc::dealloc_bytes(phys, num_lbas * lba_size) };
        result
    }

    /// Resolve a logical file block number to a physical disk block number.
    /// Returns 0 if the block is sparse (not allocated).
    pub fn resolve_block(&self, raw_inode: &Ext2Inode, logical_block: u64) -> EResult<u64> {
        let block_size = self.block_size;
        let ptrs_per_block = (block_size / 4) as u64;

        let lb = logical_block;

        // Direct blocks (0..11).
        if lb < EXT2_NDIR_BLOCKS as u64 {
            return Ok(raw_inode.i_block[lb as usize] as u64);
        }

        let lb = lb - EXT2_NDIR_BLOCKS as u64;

        // Single indirect.
        if lb < ptrs_per_block {
            let ind_block = raw_inode.i_block[EXT2_IND_BLOCK] as u64;
            if ind_block == 0 {
                return Ok(0);
            }
            return self.read_block_ptr(ind_block, lb as usize);
        }

        let lb = lb - ptrs_per_block;

        // Double indirect.
        if lb < ptrs_per_block * ptrs_per_block {
            let dind_block = raw_inode.i_block[EXT2_DIND_BLOCK] as u64;
            if dind_block == 0 {
                return Ok(0);
            }
            let ind_index = lb / ptrs_per_block;
            let ind_block = self.read_block_ptr(dind_block, ind_index as usize)?;
            if ind_block == 0 {
                return Ok(0);
            }
            let direct_index = lb % ptrs_per_block;
            return self.read_block_ptr(ind_block, direct_index as usize);
        }

        let lb = lb - ptrs_per_block * ptrs_per_block;

        // Triple indirect.
        if lb < ptrs_per_block * ptrs_per_block * ptrs_per_block {
            let tind_block = raw_inode.i_block[EXT2_TIND_BLOCK] as u64;
            if tind_block == 0 {
                return Ok(0);
            }
            let dind_index = lb / (ptrs_per_block * ptrs_per_block);
            let dind_block = self.read_block_ptr(tind_block, dind_index as usize)?;
            if dind_block == 0 {
                return Ok(0);
            }
            let remainder = lb % (ptrs_per_block * ptrs_per_block);
            let ind_index = remainder / ptrs_per_block;
            let ind_block = self.read_block_ptr(dind_block, ind_index as usize)?;
            if ind_block == 0 {
                return Ok(0);
            }
            let direct_index = remainder % ptrs_per_block;
            return self.read_block_ptr(ind_block, direct_index as usize);
        }

        Err(Errno::EFBIG)
    }

    /// Read a single u32 block pointer from a block on disk.
    fn read_block_ptr(&self, block: u64, index: usize) -> EResult<u64> {
        let mut buf = zinnia::alloc::vec![0u8; self.block_size];
        self.read_block(block, &mut buf)?;

        let offset = index * 4;
        if offset + 4 > buf.len() {
            return Err(Errno::EINVAL);
        }

        let ptr = u32::from_le_bytes([
            buf[offset],
            buf[offset + 1],
            buf[offset + 2],
            buf[offset + 3],
        ]);
        Ok(ptr as u64)
    }

    /// Allocate a new block from the block bitmap.
    pub fn alloc_block(&self, preferred_group: u32) -> EResult<u64> {
        let mut bgdt = self.bgdt.lock();
        let group_count = bgdt.len();

        for offset in 0..group_count {
            let group = (preferred_group as usize + offset) % group_count;
            if bgdt[group].bg_free_blocks_count == 0 {
                continue;
            }

            let bitmap_block = bgdt[group].bg_block_bitmap as u64;
            let mut bitmap = zinnia::alloc::vec![0u8; self.block_size];
            self.read_block(bitmap_block, &mut bitmap)?;

            for byte_idx in 0..self.block_size {
                if bitmap[byte_idx] == 0xFF {
                    continue;
                }
                for bit in 0..8u32 {
                    if bitmap[byte_idx] & (1 << bit) == 0 {
                        // Found a free block.
                        bitmap[byte_idx] |= 1 << bit;
                        self.write_block(bitmap_block, &bitmap)?;

                        bgdt[group].bg_free_blocks_count -= 1;

                        let block_num = group as u64 * self.blocks_per_group as u64
                            + self.raw.lock().s_first_data_block as u64
                            + byte_idx as u64 * 8
                            + bit as u64;

                        self.raw.lock().s_free_blocks_count -= 1;
                        return Ok(block_num);
                    }
                }
            }
        }

        Err(Errno::ENOSPC)
    }

    /// Allocate a new inode from the inode bitmap.
    pub fn alloc_inode(&self, preferred_group: u32) -> EResult<u32> {
        let mut bgdt = self.bgdt.lock();
        let group_count = bgdt.len();

        for offset in 0..group_count {
            let group = (preferred_group as usize + offset) % group_count;
            if bgdt[group].bg_free_inodes_count == 0 {
                continue;
            }

            let bitmap_block = bgdt[group].bg_inode_bitmap as u64;
            let mut bitmap = zinnia::alloc::vec![0u8; self.block_size];
            self.read_block(bitmap_block, &mut bitmap)?;

            for byte_idx in 0..self.block_size {
                if bitmap[byte_idx] == 0xFF {
                    continue;
                }
                for bit in 0..8u32 {
                    if bitmap[byte_idx] & (1 << bit) == 0 {
                        bitmap[byte_idx] |= 1 << bit;
                        self.write_block(bitmap_block, &bitmap)?;

                        bgdt[group].bg_free_inodes_count -= 1;

                        let ino =
                            group as u32 * self.inodes_per_group + byte_idx as u32 * 8 + bit + 1;

                        self.raw.lock().s_free_inodes_count -= 1;
                        return Ok(ino);
                    }
                }
            }
        }

        Err(Errno::ENOSPC)
    }

    /// Free a block in the block bitmap.
    pub fn free_block(&self, block: u64) -> EResult<()> {
        let raw = self.raw.lock();
        let relative = block - raw.s_first_data_block as u64;
        let group = (relative / self.blocks_per_group as u64) as usize;
        let index = (relative % self.blocks_per_group as u64) as usize;
        drop(raw);

        let mut bgdt = self.bgdt.lock();
        let bitmap_block = bgdt[group].bg_block_bitmap as u64;
        let mut bitmap = zinnia::alloc::vec![0u8; self.block_size];
        self.read_block(bitmap_block, &mut bitmap)?;

        let byte_idx = index / 8;
        let bit = index % 8;
        bitmap[byte_idx] &= !(1 << bit);
        self.write_block(bitmap_block, &bitmap)?;

        bgdt[group].bg_free_blocks_count += 1;
        self.raw.lock().s_free_blocks_count += 1;

        Ok(())
    }

    /// Free an inode in the inode bitmap.
    pub fn free_inode(&self, ino: u32) -> EResult<()> {
        let group = ((ino - 1) / self.inodes_per_group) as usize;
        let index = ((ino - 1) % self.inodes_per_group) as usize;

        let mut bgdt = self.bgdt.lock();
        let bitmap_block = bgdt[group].bg_inode_bitmap as u64;
        let mut bitmap = zinnia::alloc::vec![0u8; self.block_size];
        self.read_block(bitmap_block, &mut bitmap)?;

        let byte_idx = index / 8;
        let bit = index % 8;
        bitmap[byte_idx] &= !(1 << bit);
        self.write_block(bitmap_block, &bitmap)?;

        bgdt[group].bg_free_inodes_count += 1;
        self.raw.lock().s_free_inodes_count += 1;

        Ok(())
    }

    /// Convert a raw ext2 inode to a VFS INode.
    pub fn inode_to_vfs(self: &Arc<Self>, ino: u32, raw: &Ext2Inode) -> EResult<Arc<INode>> {
        let mode_bits = raw.i_mode & 0o7777;
        let file_type = raw.i_mode & S_IFMT;

        let node_ops = match file_type {
            S_IFREG => {
                let reg = Arc::new(Ext2Regular::new(self.clone(), ino));
                NodeOps::Regular(reg)
            }
            S_IFDIR => {
                let dir = Arc::new(Ext2Dir::new(self.clone(), ino));
                NodeOps::Directory(dir)
            }
            S_IFLNK => {
                let sym = Arc::new(Ext2Symlink::new(self.clone(), ino, raw));
                NodeOps::SymbolicLink(sym)
            }
            _ => return Err(Errno::ENOTSUP),
        };

        Ok(Arc::new(INode {
            node_ops,
            sb: Some(self.clone()),
            id: ino as usize,
            size: SpinMutex::new(raw.size() as usize),
            uid: SpinMutex::new(raw.i_uid as _),
            gid: SpinMutex::new(raw.i_gid as _),
            atime: SpinMutex::new(timespec {
                tv_sec: raw.i_atime as _,
                tv_nsec: 0,
            }),
            mtime: SpinMutex::new(timespec {
                tv_sec: raw.i_mtime as _,
                tv_nsec: 0,
            }),
            ctime: SpinMutex::new(timespec {
                tv_sec: raw.i_ctime as _,
                tv_nsec: 0,
            }),
            mode: SpinMutex::new(Mode::from_bits_truncate(mode_bits as u32)),
        }))
    }

    /// Allocate a block for a given inode at a logical block position.
    /// Updates the inode's block pointers on disk.
    pub fn alloc_block_for_inode(
        &self,
        ino: u32,
        raw: &mut Ext2Inode,
        logical_block: u64,
    ) -> EResult<u64> {
        let preferred_group = (ino - 1) / self.inodes_per_group;
        let new_block = self.alloc_block(preferred_group)?;

        // Zero the new block.
        let zeros = zinnia::alloc::vec![0u8; self.block_size];
        self.write_block(new_block, &zeros)?;

        let ptrs_per_block = (self.block_size / 4) as u64;

        if logical_block < EXT2_NDIR_BLOCKS as u64 {
            raw.i_block[logical_block as usize] = new_block as u32;
        } else {
            let lb = logical_block - EXT2_NDIR_BLOCKS as u64;
            if lb < ptrs_per_block {
                // Single indirect.
                if raw.i_block[EXT2_IND_BLOCK] == 0 {
                    let ind = self.alloc_block(preferred_group)?;
                    let z = zinnia::alloc::vec![0u8; self.block_size];
                    self.write_block(ind, &z)?;
                    raw.i_block[EXT2_IND_BLOCK] = ind as u32;
                }
                self.write_block_ptr(raw.i_block[EXT2_IND_BLOCK] as u64, lb as usize, new_block)?;
            } else {
                // Double/triple indirect: not needed for basic use. Return error.
                self.free_block(new_block)?;
                return Err(Errno::EFBIG);
            }
        }

        raw.i_blocks += (self.block_size / 512) as u32;
        self.write_inode(ino, raw)?;

        Ok(new_block)
    }

    /// Write a single u32 block pointer into a block on disk.
    fn write_block_ptr(&self, block: u64, index: usize, value: u64) -> EResult<()> {
        let mut buf = zinnia::alloc::vec![0u8; self.block_size];
        self.read_block(block, &mut buf)?;

        let offset = index * 4;
        if offset + 4 > buf.len() {
            return Err(Errno::EINVAL);
        }

        let bytes = (value as u32).to_le_bytes();
        buf[offset..offset + 4].copy_from_slice(&bytes);
        self.write_block(block, &buf)
    }

    /// Write the superblock and BGDT back to disk.
    pub fn sync_metadata(&self) -> EResult<()> {
        // Write superblock at byte offset 1024.
        let lba_size = self.device.get_lba_size();
        let raw = self.raw.lock();
        let sb_bytes = unsafe {
            slice::from_raw_parts(
                &*raw as *const Ext2SuperBlock as *const u8,
                size_of::<Ext2SuperBlock>(),
            )
        };

        // Read the first 2 LBAs, patch superblock at offset 1024, write back.
        let buf_size = 2048usize.max(lba_size).div_ceil(lba_size) * lba_size;
        let buf = KernelAlloc::alloc_bytes(buf_size, AllocFlags::empty())?;
        let result = (|| -> EResult<()> {
            let num_lbas = buf_size / lba_size;
            self.device.read_lba(buf, num_lbas, 0)?;

            let data: &mut [u8] = unsafe { slice::from_raw_parts_mut(buf.as_hhdm(), buf_size) };
            let copy_len = sb_bytes.len().min(data.len() - 1024);
            data[1024..1024 + copy_len].copy_from_slice(&sb_bytes[..copy_len]);

            for i in 0..num_lbas {
                let w = PhysAddr::new(buf.value() + i * lba_size);
                self.device.write_lba(w, i as u64)?;
            }
            Ok(())
        })();

        unsafe { KernelAlloc::dealloc_bytes(buf, buf_size) };
        drop(raw);
        result?;

        // Write BGDT.
        let bgdt = self.bgdt.lock();
        let bgdt_block = self.raw.lock().s_first_data_block as u64 + 1;
        let bgdt_bytes_total = bgdt.len() * size_of::<Ext2BlockGroupDesc>();
        let mut bgdt_buf = zinnia::alloc::vec![0u8; bgdt_bytes_total];
        for (i, desc) in bgdt.iter().enumerate() {
            let off = i * size_of::<Ext2BlockGroupDesc>();
            let desc_bytes = unsafe {
                slice::from_raw_parts(
                    desc as *const Ext2BlockGroupDesc as *const u8,
                    size_of::<Ext2BlockGroupDesc>(),
                )
            };
            bgdt_buf[off..off + size_of::<Ext2BlockGroupDesc>()].copy_from_slice(desc_bytes);
        }

        // Write the BGDT blocks.
        let blocks_needed = bgdt_bytes_total.div_ceil(self.block_size);
        for b in 0..blocks_needed {
            let start = b * self.block_size;
            let end = (start + self.block_size).min(bgdt_buf.len());
            let mut block_buf = zinnia::alloc::vec![0u8; self.block_size];
            block_buf[..end - start].copy_from_slice(&bgdt_buf[start..end]);
            self.write_block(bgdt_block + b as u64, &block_buf)?;
        }

        Ok(())
    }
}

impl SuperBlock for Ext2Super {
    fn sync(self: Arc<Self>) -> EResult<()> {
        self.sync_metadata()
    }

    fn statvfs(self: Arc<Self>) -> EResult<statvfs> {
        let raw = self.raw.lock();
        Ok(statvfs {
            f_bsize: self.block_size,
            f_frsize: self.block_size,
            f_blocks: raw.s_blocks_count as _,
            f_bfree: raw.s_free_blocks_count as _,
            f_bavail: raw.s_free_blocks_count.saturating_sub(raw.s_r_blocks_count) as _,
            f_files: raw.s_inodes_count as _,
            f_ffree: raw.s_free_inodes_count as _,
            f_favail: raw.s_free_inodes_count as _,
            f_fsid: 0,
            f_flag: 0,
            f_namemax: 255,
            f_basetype: {
                let mut buf = [0u8; 80];
                buf[..4].copy_from_slice(b"ext2");
                buf
            },
        })
    }
}

pub fn main(_cmdline: &str) {
    zinnia::vfs::fs::register(&Ext2Fs);
}

zinnia::module!("Ext2 File System", "Marvin Friedrich", main);
