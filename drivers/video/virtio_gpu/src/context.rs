use crate::{
    command::{ContextResource, CreateContext},
    error::GpuError,
    queue::{ControlQueue, Fencing, Retired},
};
use zinnia::{
    alloc::{collections::BTreeSet, sync::Arc},
    util::mutex::spin::SpinMutex,
};

pub struct RenderContext {
    id: u32,
    queue: Arc<ControlQueue>,
    attached: SpinMutex<BTreeSet<u32>>,
}

impl RenderContext {
    pub fn create(queue: Arc<ControlQueue>, id: u32, name: &str) -> Result<Self, GpuError> {
        let command = CreateContext::new(name);
        queue.execute_checked(id, &command, Fencing::Unfenced)?;

        Ok(Self {
            id,
            queue,
            attached: SpinMutex::new(BTreeSet::new()),
        })
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn attach(&self, resource_id: u32) -> Result<(), GpuError> {
        if !self.attached.lock().insert(resource_id) {
            return Ok(());
        }

        let command = ContextResource {
            attach: true,
            resource_id,
        };
        match self
            .queue
            .execute_checked(self.id, &command, Fencing::Unfenced)
        {
            Ok(_) => Ok(()),
            Err(error) => {
                self.attached.lock().remove(&resource_id);
                Err(error)
            }
        }
    }
}

impl Drop for RenderContext {
    fn drop(&mut self) {
        self.queue.retire(Retired::Context { id: self.id });
    }
}
