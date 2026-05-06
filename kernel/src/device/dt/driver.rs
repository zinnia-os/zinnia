use super::{Node, TREE};
use crate::{
    posix::errno::{EResult, Errno},
    util::mutex::spin::SpinMutex,
};
use alloc::collections::{btree_map::BTreeMap, vec_deque::VecDeque};

pub struct Driver {
    /// The name of this driver.
    pub name: &'static str,
    /// The device tree nodes this driver is compatible with.
    pub compatible: &'static [&'static [u8]],
    pub probe: fn(node: &Node) -> EResult<()>,
}

impl Driver {
    pub fn register(&'static self) -> EResult<()> {
        // We need a device tree to do the compat-string matching.
        let dt = TREE.get().as_ref().ok_or(Errno::EACCES)?;

        // Insert the driver if it's not been loaded yet.
        let mut drivers = DRIVERS.lock();
        if drivers.contains_key(self.name) {
            return Err(Errno::EEXIST);
        }
        drivers.insert(self.name, self);

        // Probe the driver.
        let mut queue = VecDeque::new();

        queue.push_back(dt.root());

        while let Some(node) = queue.pop_front() {
            if let Some(compatible_prop) =
                node.properties().find(|prop| prop.name() == b"compatible")
            {
                for compat in compatible_prop.as_str() {
                    if self.compatible.contains(&compat) {
                        (self.probe)(&node)?;
                    }
                }
                // Check if any driver matches that and pass it to the driver if it does.
            }

            for child in node.nodes() {
                queue.push_back(child);
            }
        }

        Ok(())
    }
}

static DRIVERS: SpinMutex<BTreeMap<&'static str, &'static Driver>> =
    SpinMutex::new(BTreeMap::new());
