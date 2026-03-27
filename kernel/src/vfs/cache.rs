use super::inode::INode;
use crate::{
    posix::errno::{EResult, Errno},
    process::Identity,
    uapi::limits::PATH_MAX,
    util::mutex::spin::SpinMutex,
    vfs::{file::OpenFlags, fs::Mount, inode::NodeOps},
};
use alloc::{collections::btree_map::BTreeMap, string::String, sync::Arc, vec::Vec};
use core::{fmt::Debug, hint::unlikely};

#[derive(Default)]
pub enum EntryState {
    /// Entry is positive and contains a link to the inode.
    Present(Arc<INode>),
    /// Entry is negative.
    NotPresent,
    /// The entry hasn't been looked up yet.
    #[default]
    NotCached,
}

/// This struct represents an entry in the VFS.
pub struct Entry {
    /// The name of this entry.
    pub name: Vec<u8>,
    /// The underlying [`INode`] this entry is pointing to.
    pub inode: SpinMutex<EntryState>,
    /// The parent of this [`Entry`].
    /// A [`None`] value indicates that this entry is a root.
    pub parent: Option<Arc<Entry>>,
    /// Contains a map of all children of this entry.
    pub children: SpinMutex<BTreeMap<Vec<u8>, Arc<Entry>>>,
    /// A list of mounts on this entry.
    pub mounts: SpinMutex<Vec<Arc<Mount>>>,
}

impl Entry {
    pub fn new(name: &[u8], inode: Option<Arc<INode>>, parent: Option<Arc<Entry>>) -> Self {
        Entry {
            name: name.to_vec(),
            inode: SpinMutex::new(match inode {
                Some(x) => EntryState::Present(x),
                None => EntryState::NotPresent,
            }),
            parent,
            children: SpinMutex::new(BTreeMap::new()),
            mounts: SpinMutex::new(Vec::new()),
        }
    }

    pub fn get_inode(&self) -> Option<Arc<INode>> {
        let mut lock = self.inode.lock();
        match &*lock {
            EntryState::Present(inode) => Some(inode.clone()),
            EntryState::NotPresent => None,
            EntryState::NotCached => {
                // Do lookup if it wasn't cached already.
                *lock = EntryState::NotPresent;
                todo!("Lookup inode and cache")
            }
        }
    }

    pub fn set_inode(&self, inode: Arc<INode>) {
        *self.inode.lock() = EntryState::Present(inode);
    }
}

impl Debug for Entry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Entry")
            .field("name", &String::from_utf8_lossy(&self.name))
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct PathNode {
    pub mount: Arc<Mount>,
    pub entry: Arc<Entry>,
}

impl PathNode {
    pub fn mount(&self, mount: Arc<Mount>) -> EResult<()> {
        self.entry.mounts.lock().push(mount.clone());

        *mount.mount_point.lock() = Some(PathNode {
            mount: self.mount.clone(),
            entry: self.entry.clone(),
        });
        Ok(())
    }

    pub fn lookup(
        root: Self,
        start: Self,
        path: &[u8],
        identity: &Identity,
        flags: LookupFlags,
    ) -> EResult<Self> {
        if unlikely(path.is_empty()) {
            return Err(Errno::ENOENT);
        }

        // If a path starts with '/', it's an absolute path.
        // In that case, skip the first character and use the current root as a starting point.
        let (mut current_node, path) = if path.first().is_some_and(|&x| x == b'/') {
            (root.clone(), &path[1..])
        } else {
            (start, path)
        };

        // Parse each component.
        for component in path.split(|&x| x == b'/').filter(|&x| !x.is_empty()) {
            // A path may never contain a NUL terminator.
            if unlikely(component.contains(&0)) {
                return Err(Errno::EILSEQ);
            }

            current_node = current_node.resolve_symlink(root.clone(), identity, flags)?;
            let Some(inode) = current_node.entry.get_inode() else {
                return Err(Errno::ENOENT);
            };

            let NodeOps::Directory(_) = &inode.node_ops else {
                return Err(Errno::ENOTDIR);
            };

            current_node.entry.get_inode().unwrap().try_access(
                identity,
                OpenFlags::empty(),
                flags.contains(LookupFlags::UseRealId),
            )?;

            current_node = current_node.lookup_child(component)?;
        }

        if flags.contains(LookupFlags::FollowSymlinks) {
            current_node = current_node.resolve_symlink(root, identity, flags)?;
        }

        let inode = current_node.entry.get_inode();
        if flags.contains(LookupFlags::MustExist) && inode.is_none() {
            return Err(Errno::ENOENT);
        }
        if flags.contains(LookupFlags::MustNotExist) && inode.is_some() {
            return Err(Errno::EEXIST);
        }
        return Ok(current_node.clone());
    }

    pub fn lookup_child(self, name: &[u8]) -> EResult<Self> {
        // Traverse the mounts.
        let mut mount = self.mount.clone();
        let mut entry = self.entry.clone();

        'again: loop {
            for child_mnt in entry.clone().mounts.lock().iter() {
                if !Arc::ptr_eq(child_mnt, &mount) {
                    mount = child_mnt.clone();
                    entry = child_mnt.root.clone();
                    continue 'again;
                }
            }
            break;
        }

        // If this entry has already been looked up before, return that.
        if let Some(child) = entry.children.lock().get(name) {
            return Ok(Self {
                mount,
                entry: child.clone(),
            });
        }

        // If it hasn't, we have to perform a new lookup into the file system.
        let parent = entry
            .get_inode()
            .expect("This directory didn't contain an inode");

        // A lookup only makes sense on directories.
        let NodeOps::Directory(x) = &parent.node_ops else {
            return Err(Errno::ENOTDIR);
        };

        if name == b"." {
            return Ok(self);
        }

        if name == b".." {
            return match self.lookup_parent() {
                Err(e) if e == Errno::ENOENT => Ok(self),
                x => x,
            };
        }

        let child = PathNode {
            mount,
            entry: Arc::try_new(Entry {
                name: name.to_vec(),
                inode: SpinMutex::default(),
                parent: Some(entry.clone()),
                children: SpinMutex::default(),
                mounts: SpinMutex::default(),
            })?,
        };

        if let Err(e) = x.lookup(&parent, &child) {
            // Allow lookup failures so we can cache it as a negative entry.
            if e != Errno::ENOENT {
                return Err(e);
            }
            *child.entry.inode.lock() = EntryState::NotPresent;
        }

        // Insert the new entry as a child.
        entry
            .children
            .lock()
            .insert(name.to_vec(), child.entry.clone());

        return Ok(child);
    }

    pub fn lookup_parent(&self) -> EResult<Self> {
        // Get the top mount point.
        let mut mount = self.mount.clone();
        let mut entry = self.entry.clone();
        while let Some(mount_point) = mount.clone().mount_point.lock().as_ref()
            && Arc::ptr_eq(&entry, &mount.root)
        {
            mount = mount_point.mount.clone();
            entry = mount_point.entry.clone();
        }

        return Ok(Self {
            mount,
            entry: entry.parent.clone().ok_or(Errno::ENOENT)?,
        });
    }

    fn resolve_symlink(
        &self,
        root: Self,
        identity: &Identity,
        flags: LookupFlags,
    ) -> EResult<Self> {
        let mut link_buf = vec![0u8; PATH_MAX as _];
        let mut current = self.clone();
        while let Some(inode) = current.entry.get_inode()
            && let NodeOps::SymbolicLink(symlink) = &inode.node_ops
        {
            let parent = current.entry.parent.as_ref().expect("Should have a root");
            let link_length = symlink.read_link(&inode, &mut link_buf)? as usize;

            let result = Self::lookup(
                root.clone(),
                PathNode {
                    mount: self.mount.clone(),
                    entry: parent.clone(),
                },
                &link_buf[0..link_length],
                identity,
                flags,
            )?;

            if Arc::ptr_eq(&result.entry, &current.entry) {
                return Err(Errno::ELOOP);
            }
            current = result;
        }

        return Ok(current);
    }

    /// Traverses a path until it encounters a node with no mount point.
    pub fn get_mount_top(self) -> PathNode {
        let mut current = self;

        loop {
            let root = match current.mount.mount_point.lock().as_ref() {
                Some(x) => x.clone(),
                None => break,
            };
            current = root;
        }

        current
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct LookupFlags: u32 {
        const FollowSymlinks = 1 << 0;
        const MustExist = 1 << 1;
        const MustNotExist = 1 << 2;
        const UseRealId = 1 << 3;
    }
}
