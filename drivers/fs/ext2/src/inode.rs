use super::{Ext2Super, structs::*};
use zinnia::{
    alloc::{sync::Arc, vec, vec::Vec},
    arch,
    core::{fmt::Debug, num::NonZeroUsize},
    memory::{
        AddressSpace, IovecIter, PagedMemoryObject, VirtAddr, VmFlags,
        cache::{MemoryObject, Pager, PagerError},
        pmm::{AllocFlags, KernelAlloc, PageAllocator},
    },
    posix::errno::{EResult, Errno},
    process::Identity,
    uapi::{self, dirent::dirent, off_t},
    util::mutex::spin::SpinMutex,
    vfs::{
        Entry, PathNode,
        cache::EntryState,
        file::{File, FileOps, MmapFlags, OpenFlags},
        inode::{Device, DirectoryOps, INode, Mode, NodeOps, RegularOps, SymlinkOps},
    },
};

/// A pager that reads file data from an ext2 inode, resolving block pointers.
#[derive(Debug)]
pub struct Ext2FilePager {
    sb: Arc<Ext2Super>,
    ino: u32,
}

impl Ext2FilePager {
    pub fn new(sb: Arc<Ext2Super>, ino: u32) -> Self {
        Self { sb, ino }
    }
}

impl Pager for Ext2FilePager {
    fn has_page(&self, _page_index: usize) -> bool {
        true
    }

    fn try_get_page(&self, page_index: usize) -> Result<zinnia::memory::PhysAddr, PagerError> {
        let page_size = arch::virt::get_page_size();
        let phys =
            KernelAlloc::alloc(1, AllocFlags::empty()).map_err(|_| PagerError::OutOfMemory)?;

        // Zero the page first.
        let page_slice: &mut [u8] =
            unsafe { core::slice::from_raw_parts_mut(phys.as_hhdm(), page_size) };
        page_slice.fill(0);

        let raw_inode = self
            .sb
            .read_inode(self.ino)
            .map_err(|_| PagerError::IoError)?;
        let file_size = raw_inode.size() as usize;
        let file_offset = page_index * page_size;

        if file_offset >= file_size {
            // Beyond end of file, return zeroed page.
            return Ok(phys);
        }

        let block_size = self.sb.block_size;
        let blocks_per_page = page_size / block_size;
        // How many blocks to read for this page (could be fractional at end of file).
        let first_logical_block = (file_offset / block_size) as u64;

        let mut page_offset = 0;
        for b in 0..blocks_per_page.max(1) {
            let logical_block = first_logical_block + b as u64;
            let disk_block = self
                .sb
                .resolve_block(&raw_inode, logical_block)
                .map_err(|_| PagerError::IoError)?;

            if disk_block == 0 {
                // Sparse block, leave zeroed.
                page_offset += block_size;
                continue;
            }

            // Read the block into the page at the right offset.
            let copy_start = page_offset;
            let copy_end = (page_offset + block_size).min(page_size);
            let copy_len = copy_end - copy_start;

            if copy_len > 0 {
                self.sb
                    .read_block(
                        disk_block,
                        &mut page_slice[copy_start..copy_start + copy_len],
                    )
                    .map_err(|_| PagerError::IoError)?;
            }

            page_offset += block_size;
            if page_offset >= page_size {
                break;
            }
        }

        Ok(phys)
    }

    fn try_put_page(
        &self,
        address: zinnia::memory::PhysAddr,
        page_index: usize,
    ) -> Result<(), PagerError> {
        let page_size = arch::virt::get_page_size();
        let page_slice: &[u8] =
            unsafe { core::slice::from_raw_parts(address.as_hhdm(), page_size) };

        let raw_inode = self
            .sb
            .read_inode(self.ino)
            .map_err(|_| PagerError::IoError)?;
        let block_size = self.sb.block_size;
        let blocks_per_page = page_size / block_size;
        let first_logical_block = (page_index * page_size / block_size) as u64;

        let mut page_offset = 0;
        for b in 0..blocks_per_page.max(1) {
            let logical_block = first_logical_block + b as u64;
            let disk_block = self
                .sb
                .resolve_block(&raw_inode, logical_block)
                .map_err(|_| PagerError::IoError)?;

            if disk_block == 0 {
                page_offset += block_size;
                continue;
            }

            let copy_end = (page_offset + block_size).min(page_size);
            let copy_len = copy_end - page_offset;

            if copy_len > 0 {
                self.sb
                    .write_block(disk_block, &page_slice[page_offset..page_offset + copy_len])
                    .map_err(|_| PagerError::IoError)?;
            }

            page_offset += block_size;
            if page_offset >= page_size {
                break;
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct Ext2Regular {
    pub sb: Arc<Ext2Super>,
    pub ino: u32,
    pub cache: Arc<PagedMemoryObject>,
}

impl Ext2Regular {
    pub fn new(sb: Arc<Ext2Super>, ino: u32) -> Self {
        let pager = Arc::new(Ext2FilePager::new(sb.clone(), ino));
        Self {
            sb,
            ino,
            cache: Arc::new(PagedMemoryObject::new(pager)),
        }
    }
}

impl RegularOps for Ext2Regular {
    fn truncate(&self, node: &INode, new_length: u64) -> EResult<()> {
        // For simplicity, only support truncating to 0 or reducing size.
        let mut raw = self.sb.read_inode(self.ino)?;
        let old_size = raw.size();

        if new_length > old_size {
            return Err(Errno::EINVAL);
        }

        // Update size.
        raw.i_size = new_length as u32;
        if raw.i_mode & S_IFMT == S_IFREG {
            raw.i_dir_acl = (new_length >> 32) as u32;
        }

        self.sb.write_inode(self.ino, &raw)?;
        *node.size.lock() = new_length as usize;
        Ok(())
    }
}

impl FileOps for Ext2Regular {
    fn read(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        let inode = file.inode.as_ref().ok_or(Errno::EINVAL)?;
        let file_size = inode.len();

        if offset as usize >= file_size {
            return Ok(0);
        }

        let copy_size = buffer.len().min(file_size - offset as usize);
        let mut v = vec![0u8; copy_size];
        let actual = (self.cache.as_ref() as &dyn MemoryObject).read(&mut v, offset as usize);
        buffer.copy_from_slice(&v[..actual])?;

        Ok(actual as _)
    }

    fn write(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        let inode = file.inode.as_ref().ok_or(Errno::EINVAL)?;
        let write_end = offset as usize + buffer.len();

        // Ensure blocks are allocated for the write range.
        let block_size = self.sb.block_size;
        let start_block = offset / block_size as u64;
        let end_block = (write_end as u64).div_ceil(block_size as u64);

        let mut raw = self.sb.read_inode(self.ino)?;
        for lb in start_block..end_block {
            let disk_block = self.sb.resolve_block(&raw, lb)?;
            if disk_block == 0 {
                self.sb.alloc_block_for_inode(self.ino, &mut raw, lb)?;
            }
        }

        let mut v = vec![0u8; buffer.len()];
        buffer.copy_to_slice(&mut v)?;
        let actual = (self.cache.as_ref() as &dyn MemoryObject).write(&v, offset as usize);

        // Mark pages dirty.
        let page_size = arch::virt::get_page_size();
        let start_page = offset as usize / page_size;
        let end_page = (offset as usize + actual).div_ceil(page_size);
        for p in start_page..end_page {
            self.cache.mark_dirty(p);
        }

        // Update file size if needed.
        let new_size = (offset as usize + actual).max(inode.len());
        *inode.size.lock() = new_size;

        // Update on-disk size.
        raw.i_size = new_size as u32;
        if raw.i_mode & S_IFMT == S_IFREG {
            raw.i_dir_acl = (new_size as u64 >> 32) as u32;
        }
        self.sb.write_inode(self.ino, &raw)?;

        // Sync dirty pages to disk.
        self.cache.sync()?;

        Ok(actual as _)
    }

    fn mmap(
        &self,
        _file: &File,
        space: &mut AddressSpace,
        addr: VirtAddr,
        len: NonZeroUsize,
        prot: VmFlags,
        flags: MmapFlags,
        offset: uapi::off_t,
    ) -> EResult<VirtAddr> {
        let object: Arc<dyn MemoryObject> = if flags.contains(MmapFlags::Private) {
            self.cache.make_private(len, offset)?
        } else {
            self.cache.clone()
        };

        let page_size = arch::virt::get_page_size();
        let misalign = addr.value() & (page_size - 1);
        let map_address = addr - misalign;
        let backed_map_size = (len.get() + misalign + page_size - 1) & !(page_size - 1);

        space.map_object(
            object,
            map_address,
            NonZeroUsize::new(backed_map_size).unwrap(),
            prot,
            offset - misalign as isize,
        )?;
        Ok(addr)
    }
}

#[derive(Debug)]
pub struct Ext2Dir {
    pub sb: Arc<Ext2Super>,
    pub ino: u32,
}

impl Ext2Dir {
    pub fn new(sb: Arc<Ext2Super>, ino: u32) -> Self {
        Self { sb, ino }
    }

    /// Read all directory entries from the on-disk directory data.
    fn read_dir_entries(&self) -> EResult<Vec<(u32, u8, Vec<u8>)>> {
        let raw = self.sb.read_inode(self.ino)?;
        let dir_size = raw.size() as usize;
        let block_size = self.sb.block_size;
        let num_blocks = dir_size.div_ceil(block_size);

        let mut entries = Vec::new();
        let mut offset = 0usize;

        for b in 0..num_blocks {
            let disk_block = self.sb.resolve_block(&raw, b as u64)?;
            if disk_block == 0 {
                offset += block_size;
                continue;
            }

            let mut buf = vec![0u8; block_size];
            self.sb.read_block(disk_block, &mut buf)?;

            let mut pos = 0;
            while pos + size_of::<Ext2DirEntry>() <= block_size && offset + pos < dir_size {
                let entry: Ext2DirEntry = unsafe {
                    core::ptr::read_unaligned(buf.as_ptr().add(pos) as *const Ext2DirEntry)
                };

                if entry.rec_len == 0 {
                    break;
                }

                if entry.inode != 0 && entry.name_len > 0 {
                    let name_start = pos + size_of::<Ext2DirEntry>();
                    let name_end = name_start + entry.name_len as usize;
                    if name_end <= buf.len() {
                        let name = buf[name_start..name_end].to_vec();
                        entries.push((entry.inode, entry.file_type, name));
                    }
                }

                pos += entry.rec_len as usize;
            }

            offset += block_size;
        }

        Ok(entries)
    }

    /// Add a directory entry to this directory.
    fn add_entry(&self, child_ino: u32, file_type: u8, name: &[u8]) -> EResult<()> {
        let mut raw = self.sb.read_inode(self.ino)?;
        let dir_size = raw.size() as usize;
        let block_size = self.sb.block_size;
        let num_blocks = dir_size.div_ceil(block_size);

        let needed_len = (size_of::<Ext2DirEntry>() + name.len()).div_ceil(4) * 4;

        // Try to find space in existing blocks.
        for b in 0..num_blocks {
            let disk_block = self.sb.resolve_block(&raw, b as u64)?;
            if disk_block == 0 {
                continue;
            }

            let mut buf = vec![0u8; block_size];
            self.sb.read_block(disk_block, &mut buf)?;

            let mut pos = 0;
            while pos + size_of::<Ext2DirEntry>() <= block_size {
                let entry: Ext2DirEntry = unsafe {
                    core::ptr::read_unaligned(buf.as_ptr().add(pos) as *const Ext2DirEntry)
                };

                if entry.rec_len == 0 {
                    break;
                }

                let actual_len = if entry.inode != 0 {
                    (size_of::<Ext2DirEntry>() + entry.name_len as usize).div_ceil(4)
                } else {
                    0
                };

                let free_space = entry.rec_len as usize - actual_len;

                if free_space >= needed_len {
                    if entry.inode != 0 {
                        // Shrink existing entry.
                        let new_rec_len = actual_len as u16;
                        buf[pos + 4] = new_rec_len as u8;
                        buf[pos + 5] = (new_rec_len >> 8) as u8;

                        // Write new entry after it.
                        let new_pos = pos + actual_len;
                        let new_entry = Ext2DirEntry {
                            inode: child_ino,
                            rec_len: (entry.rec_len - new_rec_len) as u16,
                            name_len: name.len() as u8,
                            file_type,
                        };
                        let entry_bytes = unsafe {
                            core::slice::from_raw_parts(
                                &new_entry as *const Ext2DirEntry as *const u8,
                                size_of::<Ext2DirEntry>(),
                            )
                        };
                        buf[new_pos..new_pos + size_of::<Ext2DirEntry>()]
                            .copy_from_slice(entry_bytes);
                        buf[new_pos + size_of::<Ext2DirEntry>()
                            ..new_pos + size_of::<Ext2DirEntry>() + name.len()]
                            .copy_from_slice(name);
                    } else {
                        // Reuse this dead entry.
                        let new_entry = Ext2DirEntry {
                            inode: child_ino,
                            rec_len: entry.rec_len,
                            name_len: name.len() as u8,
                            file_type,
                        };
                        let entry_bytes = unsafe {
                            core::slice::from_raw_parts(
                                &new_entry as *const Ext2DirEntry as *const u8,
                                size_of::<Ext2DirEntry>(),
                            )
                        };
                        buf[pos..pos + size_of::<Ext2DirEntry>()].copy_from_slice(entry_bytes);
                        buf[pos + size_of::<Ext2DirEntry>()
                            ..pos + size_of::<Ext2DirEntry>() + name.len()]
                            .copy_from_slice(name);
                    }

                    self.sb.write_block(disk_block, &buf)?;
                    return Ok(());
                }

                pos += entry.rec_len as usize;
            }
        }

        // No space in existing blocks, allocate a new one.
        let new_logical = num_blocks as u64;
        let new_disk_block = self
            .sb
            .alloc_block_for_inode(self.ino, &mut raw, new_logical)?;

        let mut buf = vec![0u8; block_size];
        let new_entry = Ext2DirEntry {
            inode: child_ino,
            rec_len: block_size as u16,
            name_len: name.len() as u8,
            file_type,
        };
        let entry_bytes = unsafe {
            core::slice::from_raw_parts(
                &new_entry as *const Ext2DirEntry as *const u8,
                size_of::<Ext2DirEntry>(),
            )
        };
        buf[..size_of::<Ext2DirEntry>()].copy_from_slice(entry_bytes);
        buf[size_of::<Ext2DirEntry>()..size_of::<Ext2DirEntry>() + name.len()]
            .copy_from_slice(name);

        self.sb.write_block(new_disk_block, &buf)?;

        // Update directory size.
        let new_size = (new_logical + 1) as usize * block_size;
        raw.i_size = new_size as u32;
        self.sb.write_inode(self.ino, &raw)?;

        Ok(())
    }

    /// Remove a directory entry by name.
    fn remove_entry(&self, name: &[u8]) -> EResult<u32> {
        let raw = self.sb.read_inode(self.ino)?;
        let dir_size = raw.size() as usize;
        let block_size = self.sb.block_size;
        let num_blocks = dir_size.div_ceil(block_size);

        for b in 0..num_blocks {
            let disk_block = self.sb.resolve_block(&raw, b as u64)?;
            if disk_block == 0 {
                continue;
            }

            let mut buf = vec![0u8; block_size];
            self.sb.read_block(disk_block, &mut buf)?;

            let mut pos = 0;
            let mut prev_pos: Option<usize> = None;

            while pos + size_of::<Ext2DirEntry>() <= block_size {
                let entry: Ext2DirEntry = unsafe {
                    core::ptr::read_unaligned(buf.as_ptr().add(pos) as *const Ext2DirEntry)
                };

                if entry.rec_len == 0 {
                    break;
                }

                if entry.inode != 0 && entry.name_len as usize == name.len() {
                    let name_start = pos + size_of::<Ext2DirEntry>();
                    let entry_name = &buf[name_start..name_start + entry.name_len as usize];
                    if entry_name == name {
                        let removed_ino = entry.inode;

                        if let Some(pp) = prev_pos {
                            // Merge with previous entry.
                            let prev_rec_len = u16::from_le_bytes([buf[pp + 4], buf[pp + 5]]);
                            let new_rec_len = prev_rec_len + entry.rec_len;
                            buf[pp + 4] = new_rec_len as u8;
                            buf[pp + 5] = (new_rec_len >> 8) as u8;
                        } else {
                            // First entry in block — zero inode to mark deleted.
                            buf[pos] = 0;
                            buf[pos + 1] = 0;
                            buf[pos + 2] = 0;
                            buf[pos + 3] = 0;
                        }

                        self.sb.write_block(disk_block, &buf)?;
                        return Ok(removed_ino);
                    }
                }

                prev_pos = Some(pos);
                pos += entry.rec_len as usize;
            }
        }

        Err(Errno::ENOENT)
    }
}

impl DirectoryOps for Ext2Dir {
    fn lookup(&self, _self_node: &Arc<INode>, entry: &PathNode) -> EResult<()> {
        let name = &entry.entry.name;
        let entries = self.read_dir_entries()?;

        for (ino, _ftype, ename) in &entries {
            if ename == name {
                let vfs_inode = self.sb.get_or_load_inode(*ino)?;
                entry.entry.set_inode(vfs_inode);
                return Ok(());
            }
        }

        Err(Errno::ENOENT)
    }

    fn open(
        &self,
        node: &Arc<INode>,
        path: PathNode,
        flags: OpenFlags,
        _identity: &Identity,
    ) -> EResult<Arc<File>> {
        let file = File {
            path: Some(path),
            ops: node.file_ops(),
            inode: Some(node.clone()),
            flags: SpinMutex::new(flags),
            offset: SpinMutex::new(0),
        };
        Ok(Arc::new(file))
    }

    fn create(
        &self,
        _self_node: &Arc<INode>,
        entry: Arc<Entry>,
        mode: Mode,
        _identity: &Identity,
    ) -> EResult<()> {
        let group = (self.ino - 1) / self.sb.inodes_per_group;
        let new_ino = self.sb.alloc_inode(group)?;

        // Create the on-disk inode.
        let raw_inode = Ext2Inode {
            i_mode: S_IFREG | (mode.bits() as u16 & 0o7777),
            i_uid: 0,
            i_size: 0,
            i_atime: 0,
            i_ctime: 0,
            i_mtime: 0,
            i_dtime: 0,
            i_gid: 0,
            i_links_count: 1,
            i_blocks: 0,
            i_flags: 0,
            i_osd1: 0,
            i_block: [0; EXT2_N_BLOCKS],
            i_generation: 0,
            i_file_acl: 0,
            i_dir_acl: 0,
            i_faddr: 0,
            i_osd2: [0; 12],
        };

        self.sb.write_inode(new_ino, &raw_inode)?;

        // Add directory entry.
        self.add_entry(new_ino, EXT2_FT_REG_FILE, &entry.name)?;

        // Create VFS inode and cache it.
        let vfs_inode = self.sb.clone().inode_to_vfs(new_ino, &raw_inode)?;
        self.sb
            .inode_cache
            .lock()
            .insert(new_ino, vfs_inode.clone());
        entry.set_inode(vfs_inode);

        Ok(())
    }

    fn mkdir(
        &self,
        _self_node: &Arc<INode>,
        path: PathNode,
        mode: Mode,
        _identity: &Identity,
    ) -> EResult<()> {
        let group = (self.ino - 1) / self.sb.inodes_per_group;
        let new_ino = self.sb.alloc_inode(group)?;

        let block_size = self.sb.block_size;

        // Allocate one block for `.` and `..`.
        let mut raw_inode = Ext2Inode {
            i_mode: S_IFDIR | (mode.bits() as u16 & 0o7777),
            i_uid: 0,
            i_size: block_size as u32,
            i_atime: 0,
            i_ctime: 0,
            i_mtime: 0,
            i_dtime: 0,
            i_gid: 0,
            i_links_count: 2, // . and parent's link
            i_blocks: 0,
            i_flags: 0,
            i_osd1: 0,
            i_block: [0; EXT2_N_BLOCKS],
            i_generation: 0,
            i_file_acl: 0,
            i_dir_acl: 0,
            i_faddr: 0,
            i_osd2: [0; 12],
        };

        let data_block = self.sb.alloc_block_for_inode(new_ino, &mut raw_inode, 0)?;

        // Write `.` and `..` entries.
        let mut buf = vec![0u8; block_size];

        // `.` entry.
        let dot_entry = Ext2DirEntry {
            inode: new_ino,
            rec_len: 12,
            name_len: 1,
            file_type: EXT2_FT_DIR,
        };
        let dot_bytes = unsafe {
            core::slice::from_raw_parts(
                &dot_entry as *const Ext2DirEntry as *const u8,
                size_of::<Ext2DirEntry>(),
            )
        };
        buf[..size_of::<Ext2DirEntry>()].copy_from_slice(dot_bytes);
        buf[size_of::<Ext2DirEntry>()] = b'.';

        // `..` entry.
        let dotdot_entry = Ext2DirEntry {
            inode: self.ino,
            rec_len: (block_size - 12) as u16,
            name_len: 2,
            file_type: EXT2_FT_DIR,
        };
        let dotdot_bytes = unsafe {
            core::slice::from_raw_parts(
                &dotdot_entry as *const Ext2DirEntry as *const u8,
                size_of::<Ext2DirEntry>(),
            )
        };
        buf[12..12 + size_of::<Ext2DirEntry>()].copy_from_slice(dotdot_bytes);
        buf[12 + size_of::<Ext2DirEntry>()] = b'.';
        buf[12 + size_of::<Ext2DirEntry>() + 1] = b'.';

        self.sb.write_block(data_block, &buf)?;
        self.sb.write_inode(new_ino, &raw_inode)?;

        // Add entry in parent.
        self.add_entry(new_ino, EXT2_FT_DIR, &path.entry.name)?;

        // Increment parent link count.
        let mut parent_raw = self.sb.read_inode(self.ino)?;
        parent_raw.i_links_count += 1;
        self.sb.write_inode(self.ino, &parent_raw)?;

        // Create VFS inode.
        let vfs_inode = self.sb.clone().inode_to_vfs(new_ino, &raw_inode)?;
        self.sb
            .inode_cache
            .lock()
            .insert(new_ino, vfs_inode.clone());
        path.entry.set_inode(vfs_inode);

        Ok(())
    }

    fn symlink(
        &self,
        _self_node: &Arc<INode>,
        path: PathNode,
        target_path: &[u8],
        _identity: &Identity,
    ) -> EResult<()> {
        let group = (self.ino - 1) / self.sb.inodes_per_group;
        let new_ino = self.sb.alloc_inode(group)?;

        let mut raw_inode = Ext2Inode {
            i_mode: S_IFLNK | 0o777,
            i_uid: 0,
            i_size: target_path.len() as u32,
            i_atime: 0,
            i_ctime: 0,
            i_mtime: 0,
            i_dtime: 0,
            i_gid: 0,
            i_links_count: 1,
            i_blocks: 0,
            i_flags: 0,
            i_osd1: 0,
            i_block: [0; EXT2_N_BLOCKS],
            i_generation: 0,
            i_file_acl: 0,
            i_dir_acl: 0,
            i_faddr: 0,
            i_osd2: [0; 12],
        };

        if target_path.len() <= 60 {
            // Fast symlink: store in i_block.
            let block_bytes = unsafe {
                core::slice::from_raw_parts_mut(raw_inode.i_block.as_mut_ptr() as *mut u8, 60)
            };
            block_bytes[..target_path.len()].copy_from_slice(target_path);
        } else {
            // Slow symlink: allocate a block.
            let block = self.sb.alloc_block_for_inode(new_ino, &mut raw_inode, 0)?;
            let mut buf = vec![0u8; self.sb.block_size];
            buf[..target_path.len()].copy_from_slice(target_path);
            self.sb.write_block(block, &buf)?;
        }

        self.sb.write_inode(new_ino, &raw_inode)?;
        self.add_entry(new_ino, EXT2_FT_SYMLINK, &path.entry.name)?;

        let vfs_inode = self.sb.clone().inode_to_vfs(new_ino, &raw_inode)?;
        self.sb
            .inode_cache
            .lock()
            .insert(new_ino, vfs_inode.clone());
        path.entry.set_inode(vfs_inode);

        Ok(())
    }

    fn link(
        &self,
        _node: &Arc<INode>,
        path: &PathNode,
        target: &Arc<INode>,
        _identity: &Identity,
    ) -> EResult<()> {
        let target_ino = target.id as u32;

        // Determine file type from target.
        let ft = match &target.node_ops {
            NodeOps::Regular(_) => EXT2_FT_REG_FILE,
            NodeOps::Directory(_) => EXT2_FT_DIR,
            NodeOps::SymbolicLink(_) => EXT2_FT_SYMLINK,
            _ => EXT2_FT_UNKNOWN,
        };

        self.add_entry(target_ino, ft, &path.entry.name)?;

        // Increment link count.
        let mut raw = self.sb.read_inode(target_ino)?;
        raw.i_links_count += 1;
        self.sb.write_inode(target_ino, &raw)?;

        path.entry.set_inode(target.clone());
        Ok(())
    }

    fn unlink(
        &self,
        _self_node: &Arc<INode>,
        path: &PathNode,
        _identity: &Identity,
    ) -> EResult<()> {
        let name = &path.entry.name;
        let removed_ino = self.remove_entry(name)?;

        // Decrement link count.
        let mut raw = self.sb.read_inode(removed_ino)?;
        raw.i_links_count = raw.i_links_count.saturating_sub(1);
        self.sb.write_inode(removed_ino, &raw)?;

        // Remove from entry cache.
        *path.entry.inode.lock() = EntryState::NotPresent;

        Ok(())
    }

    fn rename(
        &self,
        _self_node: &Arc<INode>,
        path: PathNode,
        target: &Arc<INode>,
        _target_path: PathNode,
        _identity: &Identity,
    ) -> EResult<()> {
        // Simple rename: remove old entry, add new entry.
        let old_name = &path.entry.name;
        let _removed_ino = self.remove_entry(old_name)?;

        let _target_dir = match &target.node_ops {
            NodeOps::Directory(_d) => {
                // Try to downcast to Ext2Dir.
                // Since we can't easily downcast, use the target entry's parent dir.
                return Err(Errno::ENOSYS);
            }
            _ => return Err(Errno::ENOTDIR),
        };
    }

    fn mknod(
        &self,
        self_node: &Arc<INode>,
        mode: Mode,
        dev: Option<Device>,
        identity: &Identity,
    ) -> EResult<Arc<INode>> {
        let _ = (self_node, mode, dev, identity);
        Err(Errno::ENODEV)
    }

    fn get_dir_entries(
        &self,
        _self_node: &Arc<INode>,
        _entry: Arc<Entry>,
        offset: off_t,
        buffer: &mut [dirent],
        _identity: &Identity,
    ) -> EResult<usize> {
        let entries = self.read_dir_entries()?;

        let mut read = 0;
        let mut current: off_t = 0;

        for (ino, ftype, name) in &entries {
            // Skip `.` and `..` since they are handled by the VFS layer.
            if name == b"." || name == b".." {
                continue;
            }

            if current < offset {
                current += 1;
                continue;
            }

            if read >= buffer.len() {
                break;
            }

            buffer[read] = dirent {
                d_ino: *ino as _,
                d_off: current as _,
                d_reclen: size_of::<dirent>() as _,
                d_type: match *ftype {
                    EXT2_FT_REG_FILE => uapi::dirent::DT_REG,
                    EXT2_FT_DIR => uapi::dirent::DT_DIR,
                    EXT2_FT_SYMLINK => uapi::dirent::DT_LNK,
                    EXT2_FT_CHRDEV => uapi::dirent::DT_CHR,
                    EXT2_FT_BLKDEV => uapi::dirent::DT_BLK,
                    EXT2_FT_FIFO => uapi::dirent::DT_FIFO,
                    EXT2_FT_SOCK => uapi::dirent::DT_SOCK,
                    _ => uapi::dirent::DT_UNKNOWN,
                },
                d_name: [0u8; _],
            };

            let copy_len = name.len().min(buffer[read].d_name.len());
            buffer[read].d_name[..copy_len].copy_from_slice(&name[..copy_len]);

            read += 1;
            current += 1;
        }

        Ok(read)
    }
}

impl FileOps for Ext2Dir {}

#[derive(Debug)]
pub struct Ext2Symlink {
    pub sb: Arc<Ext2Super>,
    pub ino: u32,
    /// Cached target for fast symlinks.
    pub target: Vec<u8>,
}

impl Ext2Symlink {
    pub fn new(sb: Arc<Ext2Super>, ino: u32, raw: &Ext2Inode) -> Self {
        let target = if raw.is_fast_symlink() {
            raw.fast_symlink_target().to_vec()
        } else {
            // Read from block.
            let mut buf = vec![0u8; raw.size() as usize];
            if let Ok(block) = sb.resolve_block(raw, 0)
                && block != 0
            {
                let mut block_buf = vec![0u8; sb.block_size];
                let _ = sb.read_block(block, &mut block_buf);
                let len = buf.len().min(block_buf.len());
                buf[..len].copy_from_slice(&block_buf[..len]);
            }
            buf
        };

        Self { sb, ino, target }
    }
}

impl SymlinkOps for Ext2Symlink {
    fn read_link(&self, _node: &INode, buf: &mut [u8]) -> EResult<u64> {
        let copy_size = buf.len().min(self.target.len());
        buf[..copy_size].copy_from_slice(&self.target[..copy_size]);
        Ok(copy_size as u64)
    }
}

impl FileOps for Ext2Symlink {}
