pub mod driver;

use crate::{
    boot::BootInfo,
    memory::view::{MemoryView, Register},
    util::once::Once,
};
use alloc::{slice, string::String, vec::Vec};

pub struct DeviceTree<'a> {
    version: u32,
    _compat_version: u32,
    _boot_cpuid: u32,
    structs: &'a [u8],
    strings: &'a [u8],
}

impl<'a> DeviceTree<'a> {
    pub const MAGIC_VALUE: u32 = 0xD00DFEED;
    pub const MAGIC: Register<u32> = Register::new(0).with_be();
    pub const TOTAL_SIZE: Register<u32> = Register::new(4).with_be();
    pub const STRUCTS_OFFSET: Register<u32> = Register::new(8).with_be();
    pub const STRINGS_OFFSET: Register<u32> = Register::new(12).with_be();
    pub const RESERVED_OFFSET: Register<u32> = Register::new(16).with_be();
    pub const VERSION: Register<u32> = Register::new(20).with_be();
    pub const COMPAT_VERSION: Register<u32> = Register::new(24).with_be();
    pub const BOOT_CPUID: Register<u32> = Register::new(28).with_be();
    pub const STRINGS_SIZE: Register<u32> = Register::new(32).with_be();
    pub const STRUCTS_SIZE: Register<u32> = Register::new(36).with_be();
    pub const FDT_BEGIN_NODE: u32 = 0x00000001;
    pub const FDT_END_NODE: u32 = 0x00000002;
    pub const FDT_PROP: u32 = 0x00000003;
    pub const FDT_NOP: u32 = 0x00000004;
    pub const FDT_END: u32 = 0x00000009;

    pub fn try_new(data: &'a [u8]) -> Option<Self> {
        if data.read_reg(Self::MAGIC)?.value() != Self::MAGIC_VALUE {
            return None;
        }

        Some(Self {
            version: data.read_reg(Self::VERSION)?.value(),
            _compat_version: data.read_reg(Self::COMPAT_VERSION)?.value(),
            _boot_cpuid: data.read_reg(Self::BOOT_CPUID)?.value(),
            structs: &data[data.read_reg(Self::STRUCTS_OFFSET)?.value() as _..]
                [..data.read_reg(Self::STRUCTS_SIZE)?.value() as _],
            strings: &data[data.read_reg(Self::STRINGS_OFFSET)?.value() as _..]
                [..data.read_reg(Self::STRINGS_SIZE)?.value() as _],
        })
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn root(&self) -> Node<'a, '_> {
        // First tag must be FDT_BEGIN_NODE.
        assert_eq!(
            self.structs
                .read_reg(Register::<u32>::new(0).with_be())
                .unwrap()
                .value(),
            Self::FDT_BEGIN_NODE
        );

        // Parse root name.
        let mut end = 4;
        while end < self.structs.len() && self.structs[end] != 0 {
            end += 1;
        }
        let name = &self.structs[4..end];
        let start = (end + 4) & !3;
        let end = find_node_end(self.structs, start);

        Node {
            tree: self,
            name,
            start,
            end,
        }
    }

    pub fn find_node(&self, path: &[u8]) -> Option<Node<'_, '_>> {
        // Resolve aliases.
        let (path, mut node) = if *path.first()? == b'/' {
            (&path[1..], self.root())
        } else {
            let (alias, rest) =
                path.split_at(path.iter().position(|x| *x == b'/').unwrap_or(path.len()));
            let aliases = self.root().nodes().find(|x| x.name() == b"aliases")?;
            let alias_prop = aliases.properties().find(|x| x.name() == alias)?;
            (rest, self.find_node(alias_prop.as_str().next()?)?)
        };

        if path.is_empty() {
            return Some(node);
        }

        for comp in path.split(|x| *x == b'/') {
            node = node.nodes().find(|x| x.name() == comp)?;
        }

        return Some(node);
    }
}

/// A node in the device tree.
#[derive(Clone)]
pub struct Node<'a, 'b> {
    tree: &'b DeviceTree<'a>,
    name: &'a [u8],
    start: usize, // offset of first property/child
    end: usize,   // offset of matching FDT_END_NODE
}

impl<'a, 'b> Node<'a, 'b> {
    pub fn name(&self) -> &[u8] {
        self.name
    }

    pub fn nodes(&self) -> NodeIter<'a, 'b> {
        NodeIter {
            tree: self.tree,
            offset: self.start,
            end: self.end,
            depth: 0,
        }
    }

    pub fn properties(&'b self) -> PropertyIter<'a, 'b> {
        PropertyIter {
            node: self,
            offset: self.start,
        }
    }
}

/// A property inside a node.
#[derive(Clone)]
pub struct Property<'a, 'b> {
    tree: &'b DeviceTree<'a>,
    node: &'b Node<'a, 'b>,
    name: &'a [u8],
    data: &'a [u8],
}

impl<'a, 'b> Property<'a, 'b> {
    pub fn tree(&self) -> &'b DeviceTree<'a> {
        self.tree
    }

    pub fn node(&self) -> &'b Node<'a, 'b> {
        self.node
    }

    pub fn name(&self) -> &[u8] {
        self.name
    }

    pub fn data(&self) -> &[u8] {
        self.data
    }

    pub fn as_str(&self) -> impl Iterator<Item = &[u8]> {
        self.data.split(|&b| b == 0).filter(|s| !s.is_empty())
    }

    pub fn as_u32(&self) -> Option<&[u32]> {
        bytemuck::try_cast_slice(self.data).ok()
    }

    pub fn as_u64(&self) -> Option<&[u64]> {
        bytemuck::try_cast_slice(self.data).ok()
    }
}

/// Iterates over direct child nodes.
pub struct NodeIter<'a, 'b> {
    tree: &'b DeviceTree<'a>,
    offset: usize,
    end: usize,
    depth: usize,
}

impl<'a, 'b> Iterator for NodeIter<'a, 'b> {
    type Item = Node<'a, 'b>;

    fn next(&mut self) -> Option<Self::Item> {
        let structs = self.tree.structs;

        while self.offset < self.end {
            let tag = structs
                .read_reg(Register::<u32>::new(self.offset).with_be())
                .unwrap()
                .value();
            self.offset += 4;

            match tag {
                DeviceTree::FDT_PROP => {
                    let len = structs
                        .read_reg(Register::<u32>::new(self.offset).with_be())
                        .unwrap()
                        .value();
                    self.offset += 8 + ((len as usize + 3) & !3);
                }
                DeviceTree::FDT_NOP => {}
                DeviceTree::FDT_BEGIN_NODE => {
                    if self.depth == 0 {
                        // parse child name
                        let mut end = self.offset;
                        while end < structs.len() && structs[end] != 0 {
                            end += 1;
                        }
                        let name = &structs[self.offset..end];
                        let start = (end + 4) & !3;
                        let child_end = find_node_end(structs, start);

                        self.offset = child_end; // skip over child for next iteration

                        return Some(Node {
                            tree: self.tree,
                            name,
                            start,
                            end: child_end,
                        });
                    } else {
                        self.depth += 1;
                        // skip nested
                        let mut end = self.offset;
                        while end < structs.len() && structs[end] != 0 {
                            end += 1;
                        }
                        self.offset = (end + 4) & !3;
                    }
                }
                DeviceTree::FDT_END_NODE => {
                    if self.depth > 0 {
                        self.depth -= 1;
                    } else {
                        // end of parent node
                        return None;
                    }
                }
                DeviceTree::FDT_END => return None,
                _ => panic!("unknown FDT tag {:#x}", tag),
            }
        }

        None
    }
}

/// Iterates over properties of a node.
pub struct PropertyIter<'a, 'b> {
    node: &'b Node<'a, 'b>,
    offset: usize,
}

impl<'a, 'b> Iterator for PropertyIter<'a, 'b> {
    type Item = Property<'a, 'b>;

    fn next(&mut self) -> Option<Self::Item> {
        let structs = self.node.tree.structs;

        while self.offset < self.node.end {
            let tag = structs
                .read_reg(Register::<u32>::new(self.offset).with_be())
                .unwrap()
                .value();
            self.offset += 4;

            match tag {
                DeviceTree::FDT_PROP => {
                    let len = structs
                        .read_reg(Register::<u32>::new(self.offset).with_be())
                        .unwrap()
                        .value();
                    let nameoff = structs
                        .read_reg(Register::<u32>::new(self.offset + 4).with_be())
                        .unwrap()
                        .value();

                    let data_start = self.offset + 8;
                    let data_end = data_start + len as usize;
                    let data = &structs[data_start..data_end];
                    self.offset = (data_end + 3) & !3;

                    let name = get_str(self.node.tree.strings, nameoff).unwrap();

                    return Some(Property {
                        tree: self.node.tree,
                        node: self.node,
                        name,
                        data,
                    });
                }
                DeviceTree::FDT_BEGIN_NODE | DeviceTree::FDT_END_NODE | DeviceTree::FDT_END => {
                    return None;
                }
                DeviceTree::FDT_NOP => continue,
                _ => panic!("unknown FDT tag {:#x}", tag),
            }
        }

        None
    }
}

/// Find matching FDT_END_NODE for a node body.
fn find_node_end(structs: &[u8], mut offset: usize) -> usize {
    let mut depth = 0;
    while offset < structs.len() {
        let tag = structs
            .read_reg(Register::<u32>::new(offset).with_be())
            .unwrap()
            .value();
        offset += 4;

        match tag {
            DeviceTree::FDT_BEGIN_NODE => {
                depth += 1;
                // skip name
                let mut end = offset;
                while end < structs.len() && structs[end] != 0 {
                    end += 1;
                }
                offset = (end + 4) & !3;
            }
            DeviceTree::FDT_PROP => {
                let len = structs
                    .read_reg(Register::<u32>::new(offset).with_be())
                    .unwrap()
                    .value();
                offset += 8 + ((len as usize + 3) & !3);
            }
            DeviceTree::FDT_NOP => {}
            DeviceTree::FDT_END_NODE => {
                if depth == 0 {
                    return offset;
                }
                depth -= 1;
            }
            DeviceTree::FDT_END => return offset,
            _ => panic!("unknown FDT tag {:#x}", tag),
        }
    }
    panic!("unterminated FDT node");
}

/// Look up a string in the strings block.
fn get_str(strings: &[u8], off: u32) -> Option<&[u8]> {
    let mut end = off as usize;
    while end < strings.len() && strings[end] != 0 {
        end += 1;
    }
    Some(&strings[off as usize..end])
}

pub static TREE: Once<Option<DeviceTree>> = Once::new();
pub static DEVICES: Once<Vec<&Node>> = Once::new();

#[initgraph::task(
    name = "system.dt.parse-blob",
    depends = [crate::memory::MEMORY_STAGE],
)]
fn TREE_STAGE() {
    let dt = match BootInfo::get().fdt_addr {
        Some(fdt_addr) => unsafe {
            let slice = slice::from_raw_parts_mut(fdt_addr.as_hhdm(), 8);
            let len = slice.read_reg(DeviceTree::TOTAL_SIZE).unwrap().value();
            let slice = slice::from_raw_parts_mut(fdt_addr.as_hhdm(), len as _);

            DeviceTree::try_new(slice).expect("Failed to parse DTB")
        },
        None => unsafe {
            TREE.init(None);
            return;
        },
    };

    let root = dt.root();
    let model = root.properties().find(|x| x.name() == b"model").unwrap();
    log!("Running on \"{}\"", String::from_utf8_lossy(model.data()));

    let chosen = dt.find_node(b"/chosen").unwrap();
    log!(
        "stdout is: \"{}\"",
        String::from_utf8_lossy(
            chosen
                .properties()
                .find(|x| x.name() == b"stdout-path")
                .unwrap()
                .as_str()
                .next()
                .unwrap()
        )
    );

    unsafe { TREE.init(Some(dt)) };
}
