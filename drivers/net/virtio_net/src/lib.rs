#![no_std]

use virtio::{VirtQueue, VirtioDevice};
use zinnia::{
    alloc::{boxed::Box, sync::Arc, vec::Vec},
    arch,
    device::{
        net::{dev::register_nic, l2::mac::MacAddr, nic::NicDevice},
        pci::{DeviceView, Driver, PciVariant},
    },
    error,
    irq::{IrqHandler, Status},
    log,
    memory::{AllocFlags, KernelAlloc, PageAllocator, PhysAddr, Register, UnsafeMemoryView},
    posix::errno::{EResult, Errno},
    util::{event::Event, mutex::spin::SpinMutex},
};

mod spec;

/// VIRTIO_F_VERSION_1 lives at bit 32 of the feature space.
const VIRTIO_F_VERSION_1_LO: u32 = 1; // bit 0 of selector 1

/// virtio-net header is 12 bytes when MRG_RXBUF or VERSION_1 is negotiated.
const VIRTIO_NET_HDR_LEN: usize = 12;

/// Maximum supported frame size (no MTU feature negotiated: standard Ethernet).
const MAX_FRAME_LEN: usize = 1518;

/// Owns a contiguous physical page allocation. Frees on drop.
struct PhysPageAllocation {
    base_addr: PhysAddr,
    pages: usize,
}

impl PhysPageAllocation {
    fn new(pages: usize) -> EResult<Self> {
        let base_addr =
            KernelAlloc::alloc(pages, AllocFlags::empty()).map_err(|_| Errno::ENOMEM)?;
        Ok(Self { base_addr, pages })
    }

    fn phys(&self) -> PhysAddr {
        self.base_addr
    }

    fn as_hhdm<T>(&self) -> *mut T {
        self.base_addr.as_hhdm::<T>()
    }
}

impl Drop for PhysPageAllocation {
    fn drop(&mut self) {
        unsafe {
            KernelAlloc::dealloc(self.base_addr, self.pages);
        }
    }
}

struct Controller {
    virtio: SpinMutex<VirtioDevice>,
    recv_queue: SpinMutex<VirtQueue>,
    send_queue: SpinMutex<VirtQueue>,
    tx_buffers: SpinMutex<Vec<TxBuffer>>,
    /// One physical page per RX descriptor, indexed by descriptor head id.
    rx_buffers: Vec<PhysPageAllocation>,
    page_size: usize,
    rx_event: Event,
    mac: MacAddr,
}

struct TxBuffer {
    desc_id: u32,
    _buffer: PhysPageAllocation,
}

impl Controller {
    fn reap_tx(&self, queue: &mut VirtQueue) {
        while let Some((desc_id, _)) = queue.get_used() {
            queue.release_used_chain(desc_id);

            let mut buffers = self.tx_buffers.lock();
            if let Some(pos) = buffers.iter().position(|buf| buf.desc_id == desc_id) {
                buffers.swap_remove(pos);
            }
        }
    }
}

impl NicDevice for Controller {
    fn mac(&self) -> MacAddr {
        self.mac
    }

    fn recv(&self, frame: &mut [u8]) -> EResult<usize> {
        // Block on rx_event until the IRQ wakes us with a used entry to claim.
        // The guard()-then-check pattern avoids missing an already-completed
        // descriptor before we put the task to sleep.
        let (desc_id, len) = loop {
            let guard = self.rx_event.guard();
            {
                let mut queue = self.recv_queue.lock();
                if let Some(used) = queue.get_used() {
                    break used;
                }
            }
            guard.wait();
        };

        debug_assert!((desc_id as usize) < self.rx_buffers.len());
        let slot = desc_id as usize;
        let buf = &self.rx_buffers[slot];

        // The first VIRTIO_NET_HDR_LEN bytes are the virtio-net header; skip them.
        let total = (len as usize).min(self.page_size);
        let payload_len = total.saturating_sub(VIRTIO_NET_HDR_LEN);
        let n = payload_len.min(frame.len());

        unsafe {
            let payload_src = buf.as_hhdm::<u8>().add(VIRTIO_NET_HDR_LEN);
            let payload = core::slice::from_raw_parts(payload_src, n);
            frame[..n].copy_from_slice(payload);
        }

        // Re-add the same physical buffer so the queue stays full.
        let mut queue = self.recv_queue.lock();
        queue.release_used_chain(desc_id);
        queue.add_buffer(&[(buf.phys(), self.page_size, true)])?;
        self.virtio.lock().notify_queue(&queue);

        Ok(n)
    }

    fn send(&self, frame: &[u8]) -> EResult<()> {
        if frame.len() > MAX_FRAME_LEN {
            return Err(Errno::EMSGSIZE);
        }

        // One page is enough for header + max frame.
        let buf = PhysPageAllocation::new(1)?;
        let hdr = spec::VirtHeader::default();

        unsafe {
            let dst = buf.as_hhdm::<u8>();
            core::ptr::write_volatile(dst as *mut spec::VirtHeader, hdr);
            core::ptr::copy_nonoverlapping(
                frame.as_ptr(),
                dst.add(VIRTIO_NET_HDR_LEN),
                frame.len(),
            );
        }

        let total_len = VIRTIO_NET_HDR_LEN + frame.len();

        {
            let mut queue = self.send_queue.lock();
            self.reap_tx(&mut queue);
            let desc_id = queue.add_buffer(&[(buf.phys(), total_len, false)])?;
            self.tx_buffers.lock().push(TxBuffer {
                desc_id: desc_id as u32,
                _buffer: buf,
            });
            self.virtio.lock().notify_queue(&queue);
        }

        Ok(())
    }
}

struct VirtioNetIrqHandler {
    controller: Arc<Controller>,
}

impl IrqHandler for VirtioNetIrqHandler {
    fn raise(&mut self) -> Status {
        {
            let mut queue = self.controller.send_queue.lock();
            self.controller.reap_tx(&mut queue);
        }
        self.controller.rx_event.wake_all();
        Status::Handled
    }
}

fn probe(_variant: &PciVariant, mut access: DeviceView<'static>) -> EResult<()> {
    log!("Probing VirtIO NIC on {}", access.address());

    // Make sure the device can do DMA and access its MMIO BARs.
    {
        let cmd = access.access().read32(access.address(), 0x04) as u16;
        let new_cmd = cmd | (1 << 1) | (1 << 2);
        access
            .access()
            .write32(access.address(), 0x04, new_cmd as u32);
    }

    let irq_line = access.setup_msix()?;

    let mut dev = VirtioDevice::new_pci(access)?;

    let dev_features_lo = dev.get_device_features(0);
    let dev_features_hi = dev.get_device_features(1);

    let supported_lo = (spec::FeatureFlags::Mac
        | spec::FeatureFlags::MrgRxbuf
        | spec::FeatureFlags::Status)
        .bits() as u32;
    let driver_lo = dev_features_lo & supported_lo;
    let driver_hi = dev_features_hi & VIRTIO_F_VERSION_1_LO;

    dev.set_driver_features(0, driver_lo);
    dev.set_driver_features(1, driver_hi);

    log!(
        "Negotiated features lo=0x{:08x}, hi=0x{:08x}",
        driver_lo,
        driver_hi
    );
    dev.finalize_features()?;

    // Read MAC if the device offered it.
    let mut mac_bytes = [0u8; 6];
    if driver_lo & spec::FeatureFlags::Mac.bits() as u32 != 0 {
        let view = dev.device_cfg();
        for i in 0..6 {
            let reg = Register::<u8>::new(i);
            mac_bytes[i] = unsafe { view.read_reg(reg).ok_or(Errno::EINVAL)?.value() };
        }
    }
    let mac = MacAddr::new(&mac_bytes);
    log!("HW Mac address: {}", mac);

    let mut recv_queue = dev.setup_queue(0)?;
    let send_queue = dev.setup_queue(1)?;

    // Wire both queues to MSI-X vector 0 (the one we configured above) and
    // disable config-change interrupts.
    let ack_rx = dev.set_queue_msix_vector(0, 0);
    let ack_tx = dev.set_queue_msix_vector(1, 0);
    let _ = dev.set_config_msix_vector(0xFFFF);
    if ack_rx != 0 || ack_tx != 0 {
        error!(
            "Device refused MSI-X vector assignment (rx={:#x}, tx={:#x})",
            ack_rx, ack_tx
        );
        return Err(Errno::EIO);
    }

    let page_size = arch::virt::get_page_size();
    assert!(
        page_size >= MAX_FRAME_LEN + VIRTIO_NET_HDR_LEN,
        "page size too small for one RX buffer per descriptor"
    );

    // Pre-populate the RX queue with one page per descriptor.
    let qs = recv_queue.queue_size() as usize;
    let mut rx_buffers: Vec<PhysPageAllocation> = Vec::with_capacity(qs);
    for _ in 0..qs {
        let buf = PhysPageAllocation::new(1)?;
        recv_queue.add_buffer(&[(buf.phys(), page_size, true)])?;
        rx_buffers.push(buf);
    }

    dev.set_driver_ok();

    dev.notify_queue(&recv_queue);

    let controller = Arc::new(Controller {
        virtio: SpinMutex::new(dev),
        recv_queue: SpinMutex::new(recv_queue),
        send_queue: SpinMutex::new(send_queue),
        tx_buffers: SpinMutex::new(Vec::new()),
        rx_buffers,
        page_size,
        rx_event: Event::new(),
        mac,
    });

    irq_line.attach(Box::new(VirtioNetIrqHandler {
        controller: controller.clone(),
    }));
    irq_line.unmask();

    register_nic(controller)?;
    Ok(())
}

const BASE_VARIANT: PciVariant = PciVariant::new().vendor(0x1af4);

static DRIVER: Driver = Driver {
    name: "virtio_net",
    variants: &[
        BASE_VARIANT.device(0x1000).with_data(0),
        BASE_VARIANT.device(0x1041).with_data(1),
    ],
    probe,
};

zinnia::module!("VirtIO NIC driver", "Marvin Friedrich", main);

fn main(_cmdline: &str) {
    match DRIVER.register() {
        Ok(_) => (),
        Err(e) => error!("Unable to load driver: {:?}", e),
    }
}
