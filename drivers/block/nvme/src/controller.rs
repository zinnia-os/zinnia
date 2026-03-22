use crate::{
    command::IdentifyCommand,
    error::NvmeError,
    namespace::Namespace,
    queue::Queue,
    spec::{self},
};
use core::{cmp::min, slice, sync::atomic::AtomicU32};
use zinnia::{
    alloc::{
        string::{String, ToString},
        sync::Arc,
        vec::Vec,
    },
    clock,
    core::sync::atomic::Ordering,
    error, log,
    memory::{
        AllocFlags, BitValue, Field, KernelAlloc, MmioView, PageAllocator, Register,
        UnsafeMemoryView,
    },
    util::mutex::spin::SpinMutex,
};

const ADMIN_QUEUE_SIZE: usize = 32;
const IO_QUEUE_SIZE_MAX: usize = 1024;

pub struct Controller {
    regs: Arc<MmioView>,
    /// The amount of bytes between two doorbells.
    doorbell_stride: usize,
    mps_min: usize,
    /// Serial number of the controller.
    serial: SpinMutex<Option<String>>,
    /// Model name of the controller.
    model: SpinMutex<Option<String>>,
    /// Revision of the controller's firmware.
    revision: SpinMutex<Option<String>>,
    /// Queue to submit admin commands to.
    pub admin_queue: SpinMutex<Option<Queue>>,
    /// Queue to submit IO commands to.
    pub io_queue: SpinMutex<Option<Queue>>,
    /// Amount of namespaces.
    ns_count: AtomicU32,
    /// Maximum data transfer size.
    pub mdts: SpinMutex<Option<usize>>,
}

impl Controller {
    pub fn new_pci(regs: MmioView) -> Result<Arc<Self>, NvmeError> {
        // Read controller version.
        let vs = unsafe { regs.read_reg(spec::regs::VS) }.ok_or(NvmeError::MmioFailed)?;
        log!(
            "NVMe controller version {}.{}.{}",
            vs.read_field(spec::regs::vs::MJR).value(),
            vs.read_field(spec::regs::vs::MNR).value(),
            vs.read_field(spec::regs::vs::TER).value(),
        );

        let cap = unsafe { regs.read_reg(spec::regs::CAP) }.ok_or(NvmeError::MmioFailed)?;

        // Check if our host page size is supported.
        let mps_max = 1usize << (cap.read_field(spec::regs::cap::MPSMAX).value() + 12);
        let mps_min = 1usize << (cap.read_field(spec::regs::cap::MPSMIN).value() + 12);
        let page_size = zinnia::arch::virt::get_page_size();
        if mps_min > page_size && mps_max < page_size {
            error!("Host page size is not supported on this NVMe!");
            return Err(NvmeError::UnsupportedPageSize);
        }

        let doorbell_stride = 4 << cap.read_field(spec::regs::cap::DSTRD).value();

        Ok(Arc::new(Self {
            regs: Arc::new(regs),
            doorbell_stride,
            mps_min,
            admin_queue: SpinMutex::new(None),
            io_queue: SpinMutex::new(None),
            serial: SpinMutex::new(None),
            model: SpinMutex::new(None),
            revision: SpinMutex::new(None),
            ns_count: AtomicU32::new(0),
            mdts: SpinMutex::new(None),
        }))
    }

    pub fn reset(&self) -> Result<(), NvmeError> {
        // Disable the controller.
        self.set_status(false)?;

        let cap = unsafe { self.regs.read_reg(spec::regs::CAP) }.ok_or(NvmeError::MmioFailed)?;

        let queue_depth = min(
            cap.read_field(spec::regs::cap::MQES).value() as usize + 1,
            IO_QUEUE_SIZE_MAX,
        );

        // Create an admin queue first so we can create more queues.
        let mut admin_queue =
            Queue::new(self.regs.clone(), self.doorbell_stride, 0, ADMIN_QUEUE_SIZE)?;

        // Set the admin queue sizes.
        let aqa = BitValue::new(0)
            .write_field(spec::regs::aqa::ACQS, (admin_queue.get_depth() - 1) as _)
            .write_field(spec::regs::aqa::ASQS, (admin_queue.get_depth() - 1) as _);
        unsafe { self.regs.write_reg(spec::regs::AQA, aqa.value()) };

        // Set the addresses to the admin submission and completion queue.
        unsafe {
            self.regs
                .write_reg(spec::regs::ASQ, admin_queue.get_sq_addr().into());

            self.regs
                .write_reg(spec::regs::ACQ, admin_queue.get_cq_addr().into());
        }

        let cc = unsafe { self.regs.read_reg(spec::regs::CC) }
            .ok_or(NvmeError::MmioFailed)?
            .write_field(spec::regs::cc::IOCQES, spec::cq_entry::SIZE.ilog2() as u8)
            .write_field(spec::regs::cc::IOSQES, spec::sq_entry::SIZE.ilog2() as u8);

        unsafe {
            self.regs
                .write_reg(spec::regs::CC, cc.value())
                .ok_or(NvmeError::MmioFailed)?;
        }

        // Re-enable the controller.
        self.set_status(true)?;

        // Create an IO queue.
        let io_queue = Queue::new(self.regs.clone(), self.doorbell_stride, 1, queue_depth)?;
        io_queue.setup_io(&mut admin_queue)?;

        *self.admin_queue.lock() = Some(admin_queue);
        *self.io_queue.lock() = Some(io_queue);

        Ok(())
    }

    pub fn identify(&self) -> Result<(), NvmeError> {
        let identify_buffer = KernelAlloc::alloc_bytes(4096, AllocFlags::empty())
            .map_err(|_| NvmeError::AllocationFailed)?;
        let mut aq_lock = self.admin_queue.lock();
        let aq = aq_lock.as_mut().ok_or(NvmeError::MissingQueue)?;
        aq.submit_cmd(IdentifyCommand {
            buffer: identify_buffer,
            controller_id: 0, // TODO: Use a real number here
            cns: 1,
            nsid: 0,
        })?;
        let comp = aq.next_completion()?;

        // Check if the identification was successful.
        if !comp.status.is_success() {
            error!("Identification failed with status {:x}", comp.status.0);
            return Err(NvmeError::CommandFailed)?;
        }

        // Read the model strings.
        let view = unsafe { MmioView::new(identify_buffer, 4096) };
        let mut serial = [0u8; 20];
        let mut model = [0u8; 40];
        let mut fwrev = [0u8; 8];
        unsafe {
            (view.base() as *mut u8)
                .byte_add(4)
                .copy_to_nonoverlapping(serial.as_mut_ptr(), serial.len());
            (view.base() as *mut u8)
                .byte_add(24)
                .copy_to_nonoverlapping(model.as_mut_ptr(), model.len());
            (view.base() as *mut u8)
                .byte_add(64)
                .copy_to_nonoverlapping(fwrev.as_mut_ptr(), fwrev.len());
        };

        *self.serial.lock() = Some(String::from_utf8_lossy(serial.trim_ascii_end()).to_string());
        *self.model.lock() = Some(String::from_utf8_lossy(model.trim_ascii_end()).to_string());
        *self.revision.lock() = Some(String::from_utf8_lossy(fwrev.trim_ascii_end()).to_string());

        self.ns_count.store(
            unsafe { view.read_reg(Register::new(516)) }
                .ok_or(NvmeError::MmioFailed)?
                .value(),
            Ordering::Release,
        );

        let mdts = unsafe { view.read_reg(Register::<u8>::new(77)) }
            .ok_or(NvmeError::MmioFailed)?
            .value();
        if mdts != 0 {
            *self.mdts.lock() = Some(mdts as usize * self.mps_min);
        }

        unsafe { KernelAlloc::dealloc_bytes(identify_buffer, 4096) };

        Ok(())
    }

    /// Scans all existing namespaces.
    pub fn scan_namespaces(self: &Arc<Self>) -> Result<Vec<Arc<Namespace>>, NvmeError> {
        let mut namespaces = Vec::new();

        let mut aq_lock = self.admin_queue.lock();
        let aq = aq_lock.as_mut().ok_or(NvmeError::MissingQueue)?;

        let identify_buffer = KernelAlloc::alloc_bytes(4096, AllocFlags::empty())
            .map_err(|_| NvmeError::AllocationFailed)?;

        let identify_view = unsafe { MmioView::new(identify_buffer, 4096) };

        let mut nsid = 0;
        loop {
            aq.submit_cmd(IdentifyCommand {
                buffer: identify_buffer,
                controller_id: 0,
                cns: 2,
                nsid,
            })?;
            assert!(aq.next_completion()?.status.is_success());

            // Parse the identifier list until we hit an namespace value of 0.
            let ns_list = unsafe {
                slice::from_raw_parts(identify_view.base() as *const u32, 1024)
                    .iter()
                    .copied()
                    .take_while(|x| *x != 0)
                    .collect::<Vec<_>>()
            };

            for id in &ns_list {
                aq.submit_cmd(IdentifyCommand {
                    buffer: identify_buffer,
                    controller_id: 0,
                    cns: 0,
                    nsid: *id,
                })?;

                log!("Submitted identify for namespace {}", id);
                assert!(aq.next_completion()?.status.is_success());

                let lba_count = unsafe {
                    identify_view
                        .read_reg(Register::<u64>::new(0).with_le())
                        .ok_or(NvmeError::MmioFailed)?
                };

                // To get the block size of a namespace, we need to read a descriptor (see LBAF0-15).
                // Get the format descriptor index used by this namespace.
                const FLBAS: Register<u8> = Register::new(26).with_le();
                const LBAF_IDX: Field<u8, u8> = Field::new_bits(FLBAS, 0..=3);
                const LBAF0: Register<u64> = Register::new(128).with_le();
                const LBADS: Field<u64, u8> = Field::new_bits(LBAF0, 16..=23);

                let lba_format_idx = unsafe {
                    identify_view
                        .read_reg(FLBAS)
                        .ok_or(NvmeError::MmioFailed)?
                        .read_field(LBAF_IDX)
                };

                let lba_format = unsafe {
                    identify_view
                        .read_reg(
                            Register::new(LBAF0.offset() + lba_format_idx.value() as usize)
                                .with_le(),
                        )
                        .ok_or(NvmeError::MmioFailed)?
                };

                let mut lba_shift = lba_format.read_field(LBADS).value();
                // If LBADS doesn't contain a non-zero value, we need to assume a default. Use 512 bytes here.
                if lba_shift == 0 {
                    lba_shift = 9;
                }

                let ns = Namespace::new(self.clone(), *id, lba_shift, lba_count.value());
                namespaces.push(Arc::new(ns));
            }

            // If there is no 0 in the buffer, there's more entries than can fit in `identify_buffer`.
            if ns_list.len() < 1024 {
                break;
            }
            nsid = *ns_list.last().unwrap();
        }

        unsafe { KernelAlloc::dealloc_bytes(identify_buffer, 4096) };

        Ok(namespaces)
    }

    /// Sets the CC.EN flag.
    fn set_status(&self, enable: bool) -> Result<(), NvmeError> {
        unsafe {
            let cc = self
                .regs
                .read_reg(spec::regs::CC)
                .ok_or(NvmeError::MmioFailed)?
                .write_field(spec::regs::cc::EN, if enable { 1 } else { 0 });
            self.regs
                .write_reg(spec::regs::CC, cc.value())
                .ok_or(NvmeError::MmioFailed)?;
        }

        clock::block_ns(100000).unwrap();
        Ok(())
    }
}
