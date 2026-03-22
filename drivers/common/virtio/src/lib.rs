#![no_std]

use zinnia::{
    core::sync::atomic::{self, Ordering},
    log,
    memory::{MmioView, PhysAddr, Register, UnsafeMemoryView},
    posix::errno::{EResult, Errno},
    system::pci::{DeviceView, PciBar},
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
    pub const _MSIX_CONFIG: Register<u16> = Register::new(0x10).with_le();
    pub const NUM_QUEUES: Register<u16> = Register::new(0x12).with_le();
    pub const DEVICE_STATUS: Register<u8> = Register::new(0x14).with_le();
    pub const _CONFIG_GENERATION: Register<u8> = Register::new(0x15).with_le();
    pub const QUEUE_SELECT: Register<u16> = Register::new(0x16).with_le();
    pub const QUEUE_SIZE: Register<u16> = Register::new(0x18).with_le();
    pub const _QUEUE_MSIX_VECTOR: Register<u16> = Register::new(0x1A).with_le();
    pub const QUEUE_ENABLE: Register<u16> = Register::new(0x1C).with_le();
    pub const QUEUE_NOTIFY_OFF: Register<u16> = Register::new(0x1E).with_le();
    pub const QUEUE_DESC: Register<u64> = Register::new(0x20).with_le();
    pub const QUEUE_AVAIL: Register<u64> = Register::new(0x28).with_le();
    pub const QUEUE_USED: Register<u64> = Register::new(0x30).with_le();
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

    pub const FLAGS: Register<u16> = Register::new(0x00).with_le();
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

pub struct VirtQueue {
    desc_view: MmioView,
    avail_view: MmioView,
    used_view: MmioView,
    queue_size: u16,
    next_desc: u16,
    pub last_used_idx: u16,
}

impl VirtQueue {
    /// Creates a new VirtQueue from physical addresses.
    /// # Safety
    /// The physical addresses must point to valid, properly aligned, zeroed virtqueue memory.
    pub unsafe fn new(
        queue_size: u16,
        desc_phys: PhysAddr,
        avail_phys: PhysAddr,
        used_phys: PhysAddr,
    ) -> Self {
        // Calculate sizes for each region.
        let desc_size = (queue_size as usize) * virtq_desc::SIZE;
        let avail_size = virtq_avail::RING_START + (queue_size as usize) * 2 + 2; // +2 for used_event
        let used_size = virtq_used::RING_START + (queue_size as usize) * virtq_used_elem::SIZE + 2; // +2 for avail_event

        Self {
            desc_view: unsafe { MmioView::new(desc_phys, desc_size) },
            avail_view: unsafe { MmioView::new(avail_phys, avail_size) },
            used_view: unsafe { MmioView::new(used_phys, used_size) },
            queue_size,
            next_desc: 0,
            last_used_idx: 0,
        }
    }

    /// Returns the queue size.
    pub fn queue_size(&self) -> u16 {
        self.queue_size
    }

    /// Adds a buffer chain to the virtqueue.
    /// Each buffer is a tuple of (physical address, length, is_device_writable).
    /// Returns the head descriptor index on success.
    pub fn add_buffer(&mut self, buffers: &[(PhysAddr, usize, bool)]) -> EResult<u16> {
        if buffers.is_empty() {
            return Err(Errno::EINVAL);
        }

        let head = self.next_desc;
        let queue_size = self.queue_size;

        unsafe {
            for (i, &(addr, len, write)) in buffers.iter().enumerate() {
                let desc_idx = (self.next_desc + i as u16) % queue_size;

                let mut flags = if write { VIRTQ_DESC_F_WRITE } else { 0 };
                let next = if i + 1 < buffers.len() {
                    flags |= VIRTQ_DESC_F_NEXT;
                    (desc_idx + 1) % queue_size
                } else {
                    0
                };

                self.set_desc(desc_idx, addr.value() as u64, len as u32, flags, next);
            }

            self.next_desc = (self.next_desc + buffers.len() as u16) % queue_size;

            // Write the head descriptor index to the available ring
            let avail_idx = self.read_avail_idx();
            self.write_avail_ring(avail_idx % queue_size, head);

            // Memory barrier to ensure descriptor writes are visible before idx update
            atomic::fence(Ordering::SeqCst);

            // Increment the available index
            self.write_avail_idx(avail_idx.wrapping_add(1));
        }

        Ok(head)
    }

    /// Checks if there are any used buffers available.
    pub fn has_used(&self) -> bool {
        unsafe {
            let used_idx = self.read_used_idx();
            used_idx != self.last_used_idx
        }
    }

    /// Gets the next used buffer, returning (descriptor_id, bytes_written).
    /// Returns None if no used buffers are available.
    pub fn get_used(&mut self) -> Option<(u32, u32)> {
        if !self.has_used() {
            return None;
        }

        unsafe {
            let idx = self.last_used_idx % self.queue_size;
            let (id, len) = self.read_used_elem(idx);
            self.last_used_idx = self.last_used_idx.wrapping_add(1);
            Some((id, len))
        }
    }

    pub unsafe fn set_desc(&self, index: u16, addr: u64, len: u32, flags: u16, next: u16) {
        unsafe {
            let view = self
                .desc_view
                .sub_view((index as usize) * virtq_desc::SIZE)
                .expect("Descriptor index out of bounds");

            view.write_reg(virtq_desc::ADDR, addr);
            view.write_reg(virtq_desc::LEN, len);
            view.write_reg(virtq_desc::FLAGS, flags);
            view.write_reg(virtq_desc::NEXT, next);
        }
    }

    pub unsafe fn read_avail_flags(&self) -> u16 {
        unsafe {
            self.avail_view
                .read_reg(virtq_avail::FLAGS)
                .expect("Failed to read avail flags")
                .value()
        }
    }

    pub unsafe fn read_avail_idx(&self) -> u16 {
        unsafe {
            self.avail_view
                .read_reg(virtq_avail::IDX)
                .expect("Failed to read avail idx")
                .value()
        }
    }

    pub unsafe fn write_avail_idx(&self, idx: u16) {
        unsafe {
            self.avail_view.write_reg(virtq_avail::IDX, idx);
        }
    }

    pub unsafe fn write_avail_ring(&self, ring_idx: u16, desc_head: u16) {
        unsafe {
            let offset = virtq_avail::RING_START + (ring_idx as usize) * 2;
            let ring_reg = Register::<u16>::new(offset).with_le();
            self.avail_view.write_reg(ring_reg, desc_head);
        }
    }

    pub unsafe fn read_used_flags(&self) -> u16 {
        unsafe {
            self.used_view
                .read_reg(virtq_used::FLAGS)
                .expect("Failed to read used flags")
                .value()
        }
    }

    pub unsafe fn read_used_idx(&self) -> u16 {
        unsafe {
            self.used_view
                .read_reg(virtq_used::IDX)
                .expect("Failed to read used idx")
                .value()
        }
    }

    pub unsafe fn read_used_elem(&self, ring_idx: u16) -> (u32, u32) {
        unsafe {
            let offset = virtq_used::RING_START + (ring_idx as usize) * virtq_used_elem::SIZE;
            let view = self
                .used_view
                .sub_view(offset)
                .expect("Used element index out of bounds");

            let id = view
                .read_reg(virtq_used_elem::ID)
                .expect("Failed to read used elem ID")
                .value();
            let len = view
                .read_reg(virtq_used_elem::LEN)
                .expect("Failed to read used elem LEN")
                .value();

            (id, len)
        }
    }
}

pub struct VirtioDevice {
    common_cfg: MmioView,
    notify_base: MmioView,
    notify_off_multiplier: u32,
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
            )
        };

        // Reset device
        let mut device = Self {
            common_cfg: common_cfg_view,
            notify_base: notify_view,
            notify_off_multiplier,
            device_cfg: device_cfg_view,
        };

        device.reset();
        device.set_status(VIRTIO_STATUS_ACKNOWLEDGE);
        device.set_status(VIRTIO_STATUS_DRIVER);

        Ok(device)
    }

    pub fn device_cfg(&self) -> &MmioView {
        &self.device_cfg
    }

    pub fn reset(&mut self) {
        self.set_status(0);
        while self.get_status() != 0 {
            core::hint::spin_loop();
        }
    }

    pub fn get_status(&self) -> u8 {
        unsafe {
            self.common_cfg
                .read_reg(common_cfg::DEVICE_STATUS)
                .unwrap()
                .value()
        }
    }

    pub fn set_status(&mut self, status: u8) {
        unsafe {
            self.common_cfg.write_reg(common_cfg::DEVICE_STATUS, status);
        }
    }

    pub fn add_status(&mut self, status: u8) {
        let current = self.get_status();
        self.set_status(current | status);
    }

    pub fn get_device_features(&mut self, select: u32) -> u32 {
        unsafe {
            self.common_cfg
                .write_reg(common_cfg::DEVICE_FEATURE_SELECT, select);
            self.common_cfg
                .read_reg(common_cfg::DEVICE_FEATURE)
                .unwrap()
                .value()
        }
    }

    pub fn set_driver_features(&mut self, select: u32, features: u32) {
        unsafe {
            self.common_cfg
                .write_reg(common_cfg::DRIVER_FEATURE_SELECT, select);
            self.common_cfg
                .write_reg(common_cfg::DRIVER_FEATURE, features);
        }
    }

    pub fn num_queues(&self) -> u16 {
        unsafe {
            self.common_cfg
                .read_reg(common_cfg::NUM_QUEUES)
                .unwrap()
                .value()
        }
    }

    pub fn get_queue_max_size(&mut self, queue_idx: u16) -> u16 {
        unsafe {
            self.common_cfg
                .write_reg(common_cfg::QUEUE_SELECT, queue_idx);
            self.common_cfg
                .read_reg(common_cfg::QUEUE_SIZE)
                .unwrap()
                .value()
        }
    }

    pub fn setup_queue(
        &mut self,
        queue_idx: u16,
        size: u16,
        desc: PhysAddr,
        avail: PhysAddr,
        used: PhysAddr,
    ) -> EResult<u16> {
        unsafe {
            self.common_cfg
                .write_reg(common_cfg::QUEUE_SELECT, queue_idx);
        }

        let max_size = unsafe {
            self.common_cfg
                .read_reg(common_cfg::QUEUE_SIZE)
                .unwrap()
                .value()
        };
        log!(
            "Queue {} max_size={}, requested={}",
            queue_idx,
            max_size,
            size
        );
        if size > max_size || max_size == 0 {
            zinnia::error!(
                "Invalid queue size for queue {}: max={}, requested={}",
                queue_idx,
                max_size,
                size
            );
            return Err(Errno::EINVAL);
        }

        unsafe {
            self.common_cfg.write_reg(common_cfg::QUEUE_SIZE, size);
            self.common_cfg
                .write_reg(common_cfg::QUEUE_DESC, desc.value() as u64);
            self.common_cfg
                .write_reg(common_cfg::QUEUE_AVAIL, avail.value() as u64);
            self.common_cfg
                .write_reg(common_cfg::QUEUE_USED, used.value() as u64);
            self.common_cfg.write_reg(common_cfg::QUEUE_ENABLE, 1u16);
        }

        let notify_off = unsafe {
            self.common_cfg
                .read_reg(common_cfg::QUEUE_NOTIFY_OFF)
                .unwrap()
                .value()
        };
        Ok(notify_off)
    }

    pub fn notify_queue(&self, notify_off: u16) {
        unsafe {
            let offset = (notify_off as u32 * self.notify_off_multiplier) as usize;
            let notify_reg = Register::<u16>::new(offset).with_le();
            self.notify_base.write_reg(notify_reg, 0);
        }
    }

    pub fn finalize(&mut self) -> EResult<()> {
        self.add_status(VIRTIO_STATUS_FEATURES_OK);

        if (self.get_status() & VIRTIO_STATUS_FEATURES_OK) == 0 {
            return Err(Errno::ENOTSUP);
        }

        self.add_status(VIRTIO_STATUS_DRIVER_OK);
        Ok(())
    }
}
