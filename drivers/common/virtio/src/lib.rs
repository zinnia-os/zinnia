#![no_std]

use zinnia::{
    alloc::vec::Vec,
    arch,
    core::sync::atomic::{self, Ordering},
    device::pci::{DeviceView, PciBar},
    log,
    memory::{
        AllocFlags, KernelAlloc, MmioView, PageAllocator, PhysAddr, Register, UnsafeMemoryView,
        VmCacheType,
    },
    posix::errno::{EResult, Errno},
};

pub const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
pub const VIRTIO_STATUS_DRIVER: u8 = 2;
pub const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
pub const VIRTIO_STATUS_FEATURES_OK: u8 = 8;
pub const VIRTIO_STATUS_DEVICE_NEEDS_RESET: u8 = 64;
pub const VIRTIO_STATUS_FAILED: u8 = 128;

pub const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
pub const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
pub const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
pub const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;
pub const VIRTIO_PCI_CAP_PCI_CFG: u8 = 5;

mod common_cfg {
    use zinnia::memory::Register;

    pub const DEVICE_FEATURE_SELECT: Register<u32> = Register::new(0x00).with_le();
    pub const DEVICE_FEATURE: Register<u32> = Register::new(0x04).with_le();
    pub const DRIVER_FEATURE_SELECT: Register<u32> = Register::new(0x08).with_le();
    pub const DRIVER_FEATURE: Register<u32> = Register::new(0x0C).with_le();
    pub const MSIX_CONFIG: Register<u16> = Register::new(0x10).with_le();
    pub const NUM_QUEUES: Register<u16> = Register::new(0x12).with_le();
    pub const DEVICE_STATUS: Register<u8> = Register::new(0x14).with_le();
    pub const _CONFIG_GENERATION: Register<u8> = Register::new(0x15).with_le();
    pub const QUEUE_SELECT: Register<u16> = Register::new(0x16).with_le();
    pub const QUEUE_SIZE: Register<u16> = Register::new(0x18).with_le();
    pub const QUEUE_MSIX_VECTOR: Register<u16> = Register::new(0x1A).with_le();
    pub const QUEUE_ENABLE: Register<u16> = Register::new(0x1C).with_le();
    pub const QUEUE_NOTIFY_OFF: Register<u16> = Register::new(0x1E).with_le();
    pub const QUEUE_DESC: Register<u64> = Register::new(0x20).with_le();
    pub const QUEUE_AVAIL: Register<u64> = Register::new(0x28).with_le();
    pub const QUEUE_USED: Register<u64> = Register::new(0x30).with_le();
}

mod isr_cfg {
    use zinnia::memory::Register;

    pub const STATUS: Register<u8> = Register::new(0x00).with_le();
}

mod virtq_desc {
    use zinnia::memory::Register;

    pub const SIZE: usize = 16;
    pub const ADDR: Register<u64> = Register::new(0x00).with_le();
    pub const LEN: Register<u32> = Register::new(0x08).with_le();
    pub const FLAGS: Register<u16> = Register::new(0x0C).with_le();
    pub const NEXT: Register<u16> = Register::new(0x0E).with_le();
}

mod virtq_avail {
    use zinnia::memory::Register;

    pub const FLAGS: Register<u16> = Register::new(0x00).with_le();
    pub const IDX: Register<u16> = Register::new(0x02).with_le();
    pub const RING_START: usize = 4;
}

mod virtq_used {
    use zinnia::memory::Register;

    pub const IDX: Register<u16> = Register::new(0x02).with_le();
    pub const RING_START: usize = 4;
}

mod virtq_used_elem {
    use zinnia::memory::Register;

    pub const SIZE: usize = 8;
    pub const ID: Register<u32> = Register::new(0x00).with_le();
    pub const LEN: Register<u32> = Register::new(0x04).with_le();
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct VirtioPciCap {
    cap_vndr: u8,
    cap_next: u8,
    cap_len: u8,
    cfg_type: u8,
    bar: u8,
    padding: [u8; 3],
    offset: u32,
    length: u32,
}

/// Descriptor flags
pub const VIRTQ_DESC_F_NEXT: u16 = 1;
pub const VIRTQ_DESC_F_WRITE: u16 = 2;

pub const VIRTQ_AVAIL_F_NO_INTERRUPT: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DescChain(u16);

impl DescChain {
    pub const fn head(&self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Used {
    pub chain: DescChain,
    pub len: u32,
}

pub struct VirtQueue {
    index: u16,
    desc_view: MmioView,
    avail_view: MmioView,
    used_view: MmioView,
    queue_size: u16,
    notify_offset: u16,
    free_descs: Vec<u16>,
    last_used_idx: u16,
}

impl VirtQueue {
    /// Creates a new VirtQueue from physical addresses.
    /// # Safety
    /// The physical addresses must point to valid, properly aligned, zeroed virtqueue memory.
    pub unsafe fn new(
        index: u16,
        queue_size: u16,
        notify_offset: u16,
        desc_phys: PhysAddr,
        avail_phys: PhysAddr,
        used_phys: PhysAddr,
    ) -> Self {
        // Calculate sizes for each region.
        let desc_size = (queue_size as usize) * virtq_desc::SIZE;
        let avail_size = virtq_avail::RING_START + (queue_size as usize) * 2 + 2; // +2 for used_event
        let used_size = virtq_used::RING_START + (queue_size as usize) * virtq_used_elem::SIZE + 2; // +2 for avail_event

        Self {
            index,
            desc_view: unsafe { MmioView::new(desc_phys, desc_size, VmCacheType::Uncacheable) },
            avail_view: unsafe { MmioView::new(avail_phys, avail_size, VmCacheType::Uncacheable) },
            used_view: unsafe { MmioView::new(used_phys, used_size, VmCacheType::Uncacheable) },
            queue_size,
            notify_offset,
            free_descs: (0..queue_size).rev().collect(),
            last_used_idx: 0,
        }
    }

    /// Returns this virtqueue's device-visible queue index.
    pub fn index(&self) -> u16 {
        self.index
    }

    /// Returns the queue size.
    pub fn queue_size(&self) -> u16 {
        self.queue_size
    }

    /// Adds a buffer chain to the virtqueue.
    /// Each buffer is a tuple of (physical address, length, is_device_writable).
    pub fn add_buffer(&mut self, buffers: &[(PhysAddr, usize, bool)]) -> EResult<DescChain> {
        if buffers.is_empty() {
            return Err(Errno::EINVAL);
        }
        if buffers.len() > self.free_descs.len() {
            return Err(Errno::EBUSY);
        }

        let queue_size = self.queue_size;
        let mut descs = Vec::with_capacity(buffers.len());
        for _ in buffers {
            descs.push(self.free_descs.pop().ok_or(Errno::EBUSY)?);
        }
        let head = descs[0];

        for (i, &(addr, len, write)) in buffers.iter().enumerate() {
            let desc_idx = descs[i];

            let mut flags = if write { VIRTQ_DESC_F_WRITE } else { 0 };
            let next = if i + 1 < buffers.len() {
                flags |= VIRTQ_DESC_F_NEXT;
                descs[i + 1]
            } else {
                0
            };

            self.set_desc(desc_idx, addr.value() as u64, len as u32, flags, next)
                .ok_or(Errno::EIO)?;
        }

        // Write the head descriptor index to the available ring
        let avail_idx = self.read_avail_idx().ok_or(Errno::EIO)?;
        self.write_avail_ring(avail_idx % queue_size, head)
            .ok_or(Errno::EIO)?;

        // Memory barrier to ensure descriptor writes are visible before idx update
        atomic::fence(Ordering::SeqCst);

        // Increment the available index
        self.write_avail_idx(avail_idx.wrapping_add(1))
            .ok_or(Errno::EIO)?;

        Ok(DescChain(head))
    }

    /// Returns a descriptor chain to the free list after the device has used it.
    pub fn release_used_chain(&mut self, chain: DescChain) {
        let mut desc_idx = chain.0;

        loop {
            debug_assert!(desc_idx < self.queue_size);
            let Some(flags) = self.read_desc_flags(desc_idx) else {
                break;
            };
            let Some(next) = self.read_desc_next(desc_idx) else {
                break;
            };
            self.free_descs.push(desc_idx);

            if flags & VIRTQ_DESC_F_NEXT == 0 {
                break;
            }

            desc_idx = next;
        }
    }

    /// Checks if there are any used buffers available.
    pub fn has_used(&self) -> bool {
        self.read_used_idx()
            .is_some_and(|idx| idx != self.last_used_idx)
    }

    /// Gets the next used buffer chain and the amount of bytes written to it.
    pub fn get_used(&mut self) -> Option<Used> {
        if !self.has_used() {
            return None;
        }

        atomic::fence(Ordering::SeqCst);
        let idx = self.last_used_idx % self.queue_size;
        let (id, len) = self.read_used_elem(idx)?;
        if id >= self.queue_size as u32 {
            return None;
        }
        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        Some(Used {
            chain: DescChain(id as u16),
            len,
        })
    }

    pub fn set_no_interrupt(&mut self, suppress: bool) -> Option<()> {
        let flags = if suppress {
            VIRTQ_AVAIL_F_NO_INTERRUPT
        } else {
            0
        };
        unsafe { self.avail_view.write_reg(virtq_avail::FLAGS, flags) }
    }

    fn set_desc(&self, index: u16, addr: u64, len: u32, flags: u16, next: u16) -> Option<()> {
        let view = self
            .desc_view
            .sub_view((index as usize) * virtq_desc::SIZE)?;

        unsafe {
            view.write_reg(virtq_desc::ADDR, addr)?;
            view.write_reg(virtq_desc::LEN, len)?;
            view.write_reg(virtq_desc::FLAGS, flags)?;
            view.write_reg(virtq_desc::NEXT, next)?;
        }

        Some(())
    }

    fn read_avail_idx(&self) -> Option<u16> {
        unsafe { self.avail_view.read_reg(virtq_avail::IDX) }.map(|x| x.value())
    }

    fn write_avail_idx(&self, idx: u16) -> Option<()> {
        unsafe { self.avail_view.write_reg(virtq_avail::IDX, idx) }
    }

    fn write_avail_ring(&self, ring_idx: u16, desc_head: u16) -> Option<()> {
        let offset = virtq_avail::RING_START + (ring_idx as usize) * 2;
        let ring_reg = Register::<u16>::new(offset).with_le();
        unsafe { self.avail_view.write_reg(ring_reg, desc_head) }
    }

    fn read_used_idx(&self) -> Option<u16> {
        unsafe { self.used_view.read_reg(virtq_used::IDX) }.map(|x| x.value())
    }

    fn read_used_elem(&self, ring_idx: u16) -> Option<(u32, u32)> {
        let offset = virtq_used::RING_START + (ring_idx as usize) * virtq_used_elem::SIZE;
        let view = self.used_view.sub_view(offset)?;

        unsafe {
            let id = view.read_reg(virtq_used_elem::ID)?.value();
            let len = view.read_reg(virtq_used_elem::LEN)?.value();
            Some((id, len))
        }
    }

    fn read_desc_flags(&self, index: u16) -> Option<u16> {
        let view = self
            .desc_view
            .sub_view((index as usize) * virtq_desc::SIZE)?;
        unsafe { view.read_reg(virtq_desc::FLAGS) }.map(|x| x.value())
    }

    fn read_desc_next(&self, index: u16) -> Option<u16> {
        let view = self
            .desc_view
            .sub_view((index as usize) * virtq_desc::SIZE)?;
        unsafe { view.read_reg(virtq_desc::NEXT) }.map(|x| x.value())
    }
}

pub struct VirtioDevice {
    common_cfg: MmioView,
    notify_base: MmioView,
    notify_off_multiplier: u32,
    isr_cfg: MmioView,
    device_cfg: MmioView,
}

impl VirtioDevice {
    pub fn new_pci(pci_device: DeviceView<'static>) -> EResult<Self> {
        // Find VirtIO capabilities
        let mut common_cfg: Option<(u8, u32, u32)> = None;
        let mut notify_cfg: Option<(u8, u32, u32)> = None;
        let mut notify_off_multiplier = 0;
        let mut isr_cfg: Option<(u8, u32, u32)> = None;
        let mut device_cfg: Option<(u8, u32, u32)> = None;

        // Read capabilities pointer
        let cap_offset_start = pci_device.access().read8(pci_device.address(), 0x34);
        let mut cap_offset = cap_offset_start;

        while cap_offset != 0 {
            let cap_vndr = pci_device
                .access()
                .read8(pci_device.address(), cap_offset as u32);
            if cap_vndr != 0x09 {
                // Not vendor-specific
                cap_offset = pci_device
                    .access()
                    .read8(pci_device.address(), (cap_offset + 1) as u32);
                continue;
            }

            // Read the VirtioPciCap structure
            let cap_data = VirtioPciCap {
                cap_vndr: pci_device
                    .access()
                    .read8(pci_device.address(), cap_offset as u32),
                cap_next: pci_device
                    .access()
                    .read8(pci_device.address(), (cap_offset + 1) as u32),
                cap_len: pci_device
                    .access()
                    .read8(pci_device.address(), (cap_offset + 2) as u32),
                cfg_type: pci_device
                    .access()
                    .read8(pci_device.address(), (cap_offset + 3) as u32),
                bar: pci_device
                    .access()
                    .read8(pci_device.address(), (cap_offset + 4) as u32),
                padding: [0; 3],
                offset: pci_device
                    .access()
                    .read32(pci_device.address(), (cap_offset + 8) as u32),
                length: pci_device
                    .access()
                    .read32(pci_device.address(), (cap_offset + 12) as u32),
            };

            match cap_data.cfg_type {
                VIRTIO_PCI_CAP_COMMON_CFG => {
                    common_cfg = Some((cap_data.bar, cap_data.offset, cap_data.length));
                }
                VIRTIO_PCI_CAP_NOTIFY_CFG => {
                    notify_cfg = Some((cap_data.bar, cap_data.offset, cap_data.length));
                    // Read notify_off_multiplier (next 4 bytes after VirtioPciCap)
                    notify_off_multiplier = pci_device
                        .access()
                        .read32(pci_device.address(), (cap_offset + 16) as u32);
                }
                VIRTIO_PCI_CAP_ISR_CFG => {
                    isr_cfg = Some((cap_data.bar, cap_data.offset, cap_data.length));
                }
                VIRTIO_PCI_CAP_DEVICE_CFG => {
                    device_cfg = Some((cap_data.bar, cap_data.offset, cap_data.length));
                }
                _ => {}
            }

            cap_offset = cap_data.cap_next;
        }

        let common_cfg = common_cfg.ok_or(Errno::ENODEV)?;
        let notify_cfg = notify_cfg.ok_or(Errno::ENODEV)?;
        let isr_cfg = isr_cfg.ok_or(Errno::ENODEV)?;
        let device_cfg = device_cfg.ok_or(Errno::ENODEV)?;

        log!(
            "common_cfg BAR={}, offset=0x{:x}",
            common_cfg.0,
            common_cfg.1
        );
        log!(
            "notify_cfg BAR={}, offset=0x{:x}",
            notify_cfg.0,
            notify_cfg.1
        );
        log!("isr_cfg BAR={}, offset=0x{:x}", isr_cfg.0, isr_cfg.1);
        log!(
            "device_cfg BAR={}, offset=0x{:x}",
            device_cfg.0,
            device_cfg.1
        );

        // Map BARs using MmioView
        let common_bar = pci_device.bar(common_cfg.0 as usize).ok_or(Errno::ENODEV)?;
        let (common_bar_addr, common_bar_size) = match common_bar {
            PciBar::Mmio32 { address, size, .. } => (PhysAddr::new(address as usize), size),
            PciBar::Mmio64 { address, size, .. } => (PhysAddr::new(address as usize), size),
            _ => return Err(Errno::EINVAL),
        };
        log!(
            "common_bar_addr = {:?}, size = {}",
            common_bar_addr,
            common_bar_size
        );
        let common_cfg_view = unsafe {
            MmioView::new(
                PhysAddr::new(common_bar_addr.value() + common_cfg.1 as usize),
                common_cfg.2 as usize,
                VmCacheType::Uncacheable,
            )
        };

        let notify_bar = pci_device.bar(notify_cfg.0 as usize).ok_or(Errno::ENODEV)?;
        let (notify_bar_addr, _notify_bar_size) = match notify_bar {
            PciBar::Mmio32 { address, size, .. } => (PhysAddr::new(address as usize), size),
            PciBar::Mmio64 { address, size, .. } => (PhysAddr::new(address as usize), size),
            _ => return Err(Errno::EINVAL),
        };
        let notify_view = unsafe {
            MmioView::new(
                PhysAddr::new(notify_bar_addr.value() + notify_cfg.1 as usize),
                notify_cfg.2 as usize,
                VmCacheType::Uncacheable,
            )
        };

        let isr_bar = pci_device.bar(isr_cfg.0 as usize).ok_or(Errno::ENODEV)?;
        let (isr_bar_addr, _isr_bar_size) = match isr_bar {
            PciBar::Mmio32 { address, size, .. } => (PhysAddr::new(address as usize), size),
            PciBar::Mmio64 { address, size, .. } => (PhysAddr::new(address as usize), size),
            _ => return Err(Errno::EINVAL),
        };
        let isr_cfg_view = unsafe {
            MmioView::new(
                PhysAddr::new(isr_bar_addr.value() + isr_cfg.1 as usize),
                isr_cfg.2 as usize,
                VmCacheType::Uncacheable,
            )
        };

        let device_bar = pci_device.bar(device_cfg.0 as usize).ok_or(Errno::ENODEV)?;
        let (device_bar_addr, _device_bar_size) = match device_bar {
            PciBar::Mmio32 { address, size, .. } => (PhysAddr::new(address as usize), size),
            PciBar::Mmio64 { address, size, .. } => (PhysAddr::new(address as usize), size),
            _ => return Err(Errno::EINVAL),
        };
        let device_cfg_view = unsafe {
            MmioView::new(
                PhysAddr::new(device_bar_addr.value() + device_cfg.1 as usize),
                device_cfg.2 as usize,
                VmCacheType::Uncacheable,
            )
        };

        // Reset device
        let mut device = Self {
            common_cfg: common_cfg_view,
            notify_base: notify_view,
            notify_off_multiplier,
            isr_cfg: isr_cfg_view,
            device_cfg: device_cfg_view,
        };

        device.reset()?;
        device.add_status(VIRTIO_STATUS_ACKNOWLEDGE)?;
        device.add_status(VIRTIO_STATUS_DRIVER)?;

        Ok(device)
    }

    pub fn device_cfg(&self) -> &MmioView {
        &self.device_cfg
    }

    pub fn read_config32(&self, reg: Register<u32>) -> EResult<u32> {
        unsafe { self.device_cfg.read_reg(reg) }
            .map(|x| x.value())
            .ok_or(Errno::EIO)
    }

    pub fn reset(&mut self) -> EResult<()> {
        self.set_status(0)?;
        while self.get_status()? != 0 {
            core::hint::spin_loop();
        }
        Ok(())
    }

    pub fn get_status(&self) -> EResult<u8> {
        unsafe { self.common_cfg.read_reg(common_cfg::DEVICE_STATUS) }
            .map(|x| x.value())
            .ok_or(Errno::EIO)
    }

    pub fn set_status(&mut self, status: u8) -> EResult<()> {
        unsafe { self.common_cfg.write_reg(common_cfg::DEVICE_STATUS, status) }.ok_or(Errno::EIO)
    }

    pub fn add_status(&mut self, status: u8) -> EResult<()> {
        let current = self.get_status()?;
        self.set_status(current | status)
    }

    pub fn get_device_features(&mut self, select: u32) -> EResult<u32> {
        unsafe {
            self.common_cfg
                .write_reg(common_cfg::DEVICE_FEATURE_SELECT, select)
                .ok_or(Errno::EIO)?;
            self.common_cfg
                .read_reg(common_cfg::DEVICE_FEATURE)
                .map(|x| x.value())
                .ok_or(Errno::EIO)
        }
    }

    pub fn set_driver_features(&mut self, select: u32, features: u32) -> EResult<()> {
        unsafe {
            self.common_cfg
                .write_reg(common_cfg::DRIVER_FEATURE_SELECT, select)
                .ok_or(Errno::EIO)?;
            self.common_cfg
                .write_reg(common_cfg::DRIVER_FEATURE, features)
                .ok_or(Errno::EIO)
        }
    }

    pub fn num_queues(&self) -> EResult<u16> {
        unsafe { self.common_cfg.read_reg(common_cfg::NUM_QUEUES) }
            .map(|x| x.value())
            .ok_or(Errno::EIO)
    }

    pub fn get_queue_max_size(&mut self, queue_idx: u16) -> EResult<u16> {
        unsafe {
            self.common_cfg
                .write_reg(common_cfg::QUEUE_SELECT, queue_idx)
                .ok_or(Errno::EIO)?;
            self.common_cfg
                .read_reg(common_cfg::QUEUE_SIZE)
                .map(|x| x.value())
                .ok_or(Errno::EIO)
        }
    }

    pub fn setup_queue(&mut self, queue_idx: u16) -> EResult<VirtQueue> {
        let size = self.get_queue_max_size(queue_idx)?;
        if size == 0 {
            return Err(Errno::ENODEV);
        }
        let (desc, avail, used) = Self::allocate_queue_memory(size)?;

        unsafe {
            self.common_cfg
                .write_reg(common_cfg::QUEUE_SIZE, size)
                .ok_or(Errno::EIO)?;
            self.common_cfg
                .write_reg(common_cfg::QUEUE_DESC, desc.value() as u64)
                .ok_or(Errno::EIO)?;
            self.common_cfg
                .write_reg(common_cfg::QUEUE_AVAIL, avail.value() as u64)
                .ok_or(Errno::EIO)?;
            self.common_cfg
                .write_reg(common_cfg::QUEUE_USED, used.value() as u64)
                .ok_or(Errno::EIO)?;
            self.common_cfg
                .write_reg(common_cfg::QUEUE_ENABLE, 1u16)
                .ok_or(Errno::EIO)?;
        }

        let notify_offset = unsafe { self.common_cfg.read_reg(common_cfg::QUEUE_NOTIFY_OFF) }
            .map(|x| x.value())
            .ok_or(Errno::EIO)?;
        Ok(unsafe { VirtQueue::new(queue_idx, size, notify_offset, desc, avail, used) })
    }

    fn allocate_queue_memory(queue_size: u16) -> EResult<(PhysAddr, PhysAddr, PhysAddr)> {
        let queue_size_usize = queue_size as usize;
        let page_size = arch::virt::get_page_size();

        // Descriptor table: 16 bytes per entry
        let desc_size = queue_size_usize * 16;
        let desc_pages = desc_size.div_ceil(page_size);
        let desc_addr =
            KernelAlloc::alloc(desc_pages, AllocFlags::empty()).map_err(|_| Errno::ENOMEM)?;

        // Available ring: 2 + 2 + (2 * queue_size) + 2 bytes (with padding)
        let avail_size = 6 + 2 * queue_size_usize;
        let avail_pages = avail_size.div_ceil(page_size);
        let avail_addr =
            KernelAlloc::alloc(avail_pages, AllocFlags::empty()).map_err(|_| Errno::ENOMEM)?;

        // Used ring: 2 + 2 + (8 * queue_size) + 2 bytes (with padding)
        let used_size = 6 + 8 * queue_size_usize;
        let used_pages = used_size.div_ceil(page_size);
        let used_addr =
            KernelAlloc::alloc(used_pages, AllocFlags::empty()).map_err(|_| Errno::ENOMEM)?;

        Ok((desc_addr, avail_addr, used_addr))
    }

    pub fn notify_queue(&self, queue: &VirtQueue) -> EResult<()> {
        let offset = (queue.notify_offset as u32 * self.notify_off_multiplier) as usize;
        let notify_reg = Register::<u16>::new(offset).with_le();
        unsafe { self.notify_base.write_reg(notify_reg, queue.index()) }.ok_or(Errno::EIO)
    }

    pub fn ack_interrupt(&self) -> EResult<u8> {
        unsafe { self.isr_cfg.read_reg(isr_cfg::STATUS) }
            .map(|x| x.value())
            .ok_or(Errno::EIO)
    }

    pub fn finalize_features(&mut self) -> EResult<()> {
        self.add_status(VIRTIO_STATUS_FEATURES_OK)?;

        if (self.get_status()? & VIRTIO_STATUS_FEATURES_OK) == 0 {
            return Err(Errno::ENOTSUP);
        }

        Ok(())
    }

    pub fn set_driver_ok(&mut self) -> EResult<()> {
        self.add_status(VIRTIO_STATUS_DRIVER_OK)
    }

    pub fn finalize(&mut self) -> EResult<()> {
        self.finalize_features()?;
        self.set_driver_ok()
    }

    pub fn set_config_msix_vector(&mut self, vector: u16) -> EResult<u16> {
        unsafe {
            self.common_cfg
                .write_reg(common_cfg::MSIX_CONFIG, vector)
                .ok_or(Errno::EIO)?;
            self.common_cfg
                .read_reg(common_cfg::MSIX_CONFIG)
                .map(|x| x.value())
                .ok_or(Errno::EIO)
        }
    }

    pub fn set_queue_msix_vector(&mut self, queue_idx: u16, vector: u16) -> EResult<u16> {
        unsafe {
            self.common_cfg
                .write_reg(common_cfg::QUEUE_SELECT, queue_idx)
                .ok_or(Errno::EIO)?;
            self.common_cfg
                .write_reg(common_cfg::QUEUE_MSIX_VECTOR, vector)
                .ok_or(Errno::EIO)?;
            self.common_cfg
                .read_reg(common_cfg::QUEUE_MSIX_VECTOR)
                .map(|x| x.value())
                .ok_or(Errno::EIO)
        }
    }
}
