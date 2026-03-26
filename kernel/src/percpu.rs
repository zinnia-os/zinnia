//! Per-CPU data structures.

use super::{memory::VirtAddr, sched::Scheduler};
use crate::{
    arch,
    {
        memory::{
            self,
            pmm::{AllocFlags, KernelAlloc, PageAllocator},
            virt::{VmFlags, mmu::PageTable},
        },
        posix::errno::{EResult, Errno},
    },
};
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

/// Common processor-local information.
#[derive(Debug)]
pub struct CpuData {
    /// A pointer to this exact structure.
    pub this: AtomicPtr<CpuData>,
    /// The ID of this CPU.
    pub id: usize,
    /// Stack pointer for kernel mode. Only used for task switching.
    pub kernel_stack: AtomicUsize,
    /// Stack pointer for user mode.
    pub user_stack: AtomicUsize,
    /// Whether this CPU is online.
    pub online: AtomicBool,
    /// Whether this CPU is present.
    pub present: AtomicBool,
    /// A scheduler instance on this CPU.
    pub scheduler: Scheduler,
}

impl CpuData {
    /// Gets the data for the current CPU.
    pub fn get() -> &'static CpuData {
        return unsafe { arch::cpu::get_per_cpu().as_ref().unwrap() };
    }

    /// Gets the data for a specified CPU.
    pub fn get_for(id: usize) -> Option<&'static CpuData> {
        if id >= NUM_CPUS.load(Ordering::Acquire) {
            return None;
        }

        let percpu_size = &raw const LD_PERCPU_END as usize - &raw const LD_PERCPU_START as usize;

        unsafe {
            let start = &raw const LD_PERCPU_START as *const CpuData;
            return start.byte_add(percpu_size * id).as_ref();
        }
    }

    pub fn iter() -> CpuDataIter {
        CpuDataIter { id: 0 }
    }
}

pub struct CpuDataIter {
    id: usize,
}

impl Iterator for CpuDataIter {
    type Item = &'static CpuData;

    fn next(&mut self) -> Option<Self::Item> {
        let result = CpuData::get_for(self.id);
        self.id += 1;
        result
    }
}

/// [`PerCpuData`] uses the following trick: All data is placed into a special section.
/// At first, this section only contains one instance of all fields (enough to initialize the boot CPU and
/// to trick the borrow checker). When a new CPU is allocated, the contents of this section are copied over,
/// and the memory region is extended. This is why the per-CPU section *must* always come last.
/// The actual access to this variable is done by calculating the offset of the current per-CPU context.
///
/// Do not use this directly, instead use the [`crate::per_cpu`] macro!
#[repr(transparent)]
pub struct PerCpuData<T: 'static> {
    /// This value is only used as placeholder storage. It is never directly accessed.
    storage: T,
}

unsafe extern "C" {
    pub unsafe static LD_PERCPU_START: u8;
    pub unsafe static LD_PERCPU_END: u8;
    pub unsafe static LD_PERCPU_CTORS_START: u8;
    pub unsafe static LD_PERCPU_CTORS_END: u8;
}

impl<T> PerCpuData<T> {
    pub const fn new(value: T) -> Self {
        return Self { storage: value };
    }

    /// Gets the CPU-local instance of this variable.
    #[inline]
    pub fn get(&self) -> &'static T {
        self.get_for(CpuData::get())
    }

    /// Gets the CPU-local instance of this variable for a given context.
    #[inline]
    pub fn get_for(&self, context: &'static CpuData) -> &'static T {
        unsafe {
            let start = &raw const LD_PERCPU_START as usize;
            (core::ptr::from_ref(context) as *const T)
                .byte_add(&raw const self.storage as usize - start)
                .as_ref()
                .unwrap()
        }
    }
}

pub type PerCpuCtor = fn(cpu_data: &'static CpuData);

// This variable must come first, so put it in a special section that is guaranteed to be put before `.percpu`.
#[used]
#[unsafe(link_section = ".percpu.init")]
pub static CPU_DATA: PerCpuData<CpuData> = PerCpuData::new(CpuData {
    this: AtomicPtr::new(&raw const LD_PERCPU_START as *mut CpuData),
    id: 0,
    kernel_stack: AtomicUsize::new(0),
    user_stack: AtomicUsize::new(0),
    online: AtomicBool::new(false),
    present: AtomicBool::new(false),
    scheduler: Scheduler::new(),
});

/// Counts how many CPUs have been allocated. ID 0 is always used for the BSP.
static NUM_CPUS: AtomicUsize = AtomicUsize::new(1);

/// Extends the per-CPU data for a new CPU.
/// Returns the new CpuData context.
pub(crate) fn allocate_cpu() -> EResult<&'static CpuData> {
    let id = NUM_CPUS.fetch_add(1, Ordering::Relaxed);
    let percpu_size = &raw const LD_PERCPU_END as usize - &raw const LD_PERCPU_START as usize;
    let percpu_new = &raw const LD_PERCPU_START as usize + (percpu_size * id);

    let phys = memory::pmm::KernelAlloc::alloc_bytes(percpu_size, AllocFlags::Zeroed)
        .map_err(|_| Errno::ENOMEM)?;

    PageTable::get_kernel()
        .map_range::<KernelAlloc>(
            VirtAddr::from(percpu_new),
            phys,
            VmFlags::Read | VmFlags::Write,
            percpu_size,
        )
        .map_err(|_| Errno::ENOMEM)?;

    unsafe {
        let this_ptr = percpu_new as *mut CpuData;
        this_ptr.write(CpuData {
            this: AtomicPtr::new(this_ptr),
            id,
            kernel_stack: AtomicUsize::new(0),
            user_stack: AtomicUsize::new(0),
            online: AtomicBool::new(false),
            present: AtomicBool::new(false),
            scheduler: Scheduler::new(),
        });
        let new_context = this_ptr.as_ref().unwrap();

        // We need to call functions to create default values.
        let start = &raw const LD_PERCPU_CTORS_START as *const PerCpuCtor;
        let end = &raw const LD_PERCPU_CTORS_END as *const PerCpuCtor;

        let mut ctor = start;
        while (ctor as usize) < (end as usize) {
            (*ctor)(new_context);
            ctor = ctor.add(1);
        }

        return Ok(new_context);
    }
}
