use crate::{
    command::AttachBacking,
    dma::DmaRegion,
    error::GpuError,
    queue::{ControlQueue, Fencing, Retired},
};
use core::sync::atomic::{AtomicU64, Ordering};
use zinnia::{
    alloc::{sync::Arc, vec::Vec},
    arch,
    device::drm::object::BufferObject,
    memory::{MemoryObject, PhysAddr},
    posix::errno::EResult,
};

pub struct Resource {
    id: u32,
    backing: Option<DmaRegion>,
    queue: Arc<ControlQueue>,
    last_fence: AtomicU64,
}

impl Resource {
    pub fn adopt(
        queue: Arc<ControlQueue>,
        id: u32,
        backing: Option<DmaRegion>,
    ) -> Result<Arc<Self>, GpuError> {
        let resource = Arc::new(Self {
            id,
            backing,
            queue,
            last_fence: AtomicU64::new(0),
        });

        if let Some(backing) = resource.backing.as_ref() {
            let runs = contiguous_runs(backing);
            let command = AttachBacking::new(id, &runs)?;
            resource
                .queue
                .execute_checked(0, &command, Fencing::Unfenced)?;
        }

        Ok(resource)
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn size(&self) -> usize {
        self.backing.as_ref().map_or(0, |backing| backing.len())
    }

    pub fn record_fence(&self, fence_id: u64) {
        self.last_fence.fetch_max(fence_id, Ordering::AcqRel);
    }

    pub fn last_fence(&self) -> u64 {
        self.last_fence.load(Ordering::Acquire)
    }

    pub fn is_busy(&self) -> bool {
        self.queue.signaled_fence() < self.last_fence()
    }

    pub fn wait_idle(&self) -> Result<(), GpuError> {
        self.queue.wait_fence(self.last_fence())
    }
}

impl Drop for Resource {
    fn drop(&mut self) {
        self.queue.retire(Retired::Resource {
            id: self.id,
            backing: self.backing.take(),
        });
    }
}

fn contiguous_runs(backing: &DmaRegion) -> Vec<(PhysAddr, usize)> {
    let page_size = arch::virt::get_page_size();
    let mut runs: Vec<(PhysAddr, usize)> = Vec::new();
    let mut index = 0;

    while let Some(page) = backing.page_at(index) {
        let remaining = backing.len() - index * page_size;
        let length = remaining.min(page_size);

        match runs.last_mut() {
            Some((start, run_len)) if *start + *run_len == page => *run_len += length,
            _ => runs.push((page, length)),
        }

        index += 1;
    }

    runs
}

pub struct GpuBuffer {
    handle: u32,
    resource: Arc<Resource>,
    width: u32,
    height: u32,
    dumb: bool,
}

impl GpuBuffer {
    pub fn new(handle: u32, resource: Arc<Resource>, width: u32, height: u32, dumb: bool) -> Self {
        Self {
            handle,
            resource,
            width,
            height,
            dumb,
        }
    }

    pub fn resource(&self) -> &Arc<Resource> {
        &self.resource
    }

    pub fn is_dumb(&self) -> bool {
        self.dumb
    }
}

impl MemoryObject for GpuBuffer {
    fn try_get_page(&self, page_index: usize) -> EResult<Option<PhysAddr>> {
        Ok(self
            .resource
            .backing
            .as_ref()
            .and_then(|backing| backing.page_at(page_index)))
    }
}

impl BufferObject for GpuBuffer {
    fn id(&self) -> u32 {
        self.handle
    }

    fn size(&self) -> usize {
        self.resource.size()
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }
}
