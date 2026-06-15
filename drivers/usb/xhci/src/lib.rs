#![no_std]

use zinnia::{
    alloc::{boxed::Box, format, sync::Arc, vec::Vec},
    arch, clock,
    device::{
        pci::{self, DeviceView, PciBar, PciVariant},
        usb::{self, hub::Hub},
    },
    error,
    irq::{IrqHandler, IrqLine, Status},
    log,
    memory::{
        AllocFlags, BitValue, MemoryView, MmioSubView, MmioView, OwnedPhysPages, PhysAddr,
        Register, UnsafeMemoryView, VmCacheType,
    },
    posix::errno::{EResult, Errno},
    util::mutex::spin::SpinMutex,
};

mod device;
mod hub;
mod ring;
mod spec;
mod transfer;

use device::{XhciControllerOps, XhciDevice, xhci_speed_to_usb};
use hub::XhciRootHubOps;
use ring::{Completion, CompletionCell, Ring};
use spec::{TrbType, caps, doorbell, erst_entry, interrupter, opregs, port, runtime};

zinnia::module!("xHCI USB Controller", "Marvin Friedrich", main);

const HANDSHAKE_TIMEOUT_NS: usize = 1_000_000_000;
const HANDOFF_TIMEOUT_NS: usize = 5_000_000_000;
static CONTROLLERS: SpinMutex<Vec<Arc<XhciController>>> = SpinMutex::new(Vec::new());

pub fn main(_cmdline: &str) {
    match PCI_DRIVER.register() {
        Ok(_) => (),
        Err(e) => error!("Unable to load driver: {:?}", e),
    }
}

static PCI_DRIVER: pci::Driver = pci::Driver {
    name: "xhci_pci",
    // Serial, USB, xHCI
    variants: &[PciVariant::new().class(0xC).sub_class(0x3).function(0x30)],
    probe,
};

fn probe(_variant: &PciVariant, mut view: DeviceView<'static>) -> EResult<()> {
    let address = view.address();
    log!("Setting up xHCI controller on {}", address);

    let bar = view.bar(0).ok_or(Errno::ENXIO)?;
    let (bar_addr, bar_size) = match bar {
        PciBar::Mmio32 { address, size, .. } => (address as usize, size),
        PciBar::Mmio64 { address, size, .. } => (address as usize, size),
        PciBar::Io { .. } => return Err(Errno::EINVAL),
    };

    // Enable memory decode and bus mastering, disable legacy INTx.
    let cmd = view.access().read16(address, 0x04);
    view.access().write16(
        address,
        0x04,
        (cmd | (1 << 1) | (1 << 2) | (1 << 10)) & !(1 << 0),
    );

    let mmio = Arc::new(unsafe {
        MmioView::new(PhysAddr::new(bar_addr), bar_size, VmCacheType::Uncacheable)
    });

    let caplength = unsafe { mmio.read_reg(caps::CAPLENGTH) }
        .ok_or(Errno::EIO)?
        .value() as usize;
    let rtsoff = unsafe { mmio.read_reg(caps::RTSOFF) }
        .ok_or(Errno::EIO)?
        .value() as usize;
    let dboff = unsafe { mmio.read_reg(caps::DBOFF) }
        .ok_or(Errno::EIO)?
        .value() as usize;

    let hccparams1 = unsafe { mmio.read_reg(caps::HCCPARAMS1) }.ok_or(Errno::EIO)?;
    let hcsparams1 = unsafe { mmio.read_reg(caps::HCSPARAMS1) }.ok_or(Errno::EIO)?;
    let hcsparams2 = unsafe { mmio.read_reg(caps::HCSPARAMS2) }.ok_or(Errno::EIO)?;

    // Only 64-bit-addressing controllers are supported for simplicity.
    if hccparams1.read_field(caps::hccparams1::AC64).value() == 0 {
        error!("Controller does not support 64-bit addressing");
        return Err(Errno::ENOTSUP);
    }

    let ctx_stride = if hccparams1.read_field(caps::hccparams1::CSZ).value() != 0 {
        64
    } else {
        32
    };

    let opregs_base = caplength;
    let pagesize = unsafe {
        mmio.sub_view(opregs_base)
            .ok_or(Errno::EIO)?
            .read_reg(opregs::PAGESIZE)
    }
    .ok_or(Errno::EIO)?
    .value();
    if pagesize & 0x1 == 0 {
        error!("Controller does not support 4K page size");
        return Err(Errno::ENOTSUP);
    }

    // Take ownership from the BIOS before touching anything else.
    let xecp = hccparams1.read_field(caps::hccparams1::XECP).value();
    handoff(&mmio, xecp)?;

    let max_slots = hcsparams1.read_field(caps::hcsparams1::MAX_SLOTS).value();
    let max_ports = hcsparams1.read_field(caps::hcsparams1::MAX_PORTS).value();
    log!(
        "Controller supports {} slots and {} ports",
        max_slots,
        max_ports
    );

    let scratchpad_count = {
        let hi = hcsparams2
            .read_field(caps::hcsparams2::MAX_SCRATCHPAD_HI)
            .value() as u32;
        let lo = hcsparams2
            .read_field(caps::hcsparams2::MAX_SCRATCHPAD_LO)
            .value() as u32;
        lo | (hi << 5)
    };

    let xhci = Arc::new(XhciController::new(
        mmio,
        opregs_base,
        rtsoff,
        dboff,
        max_slots,
        max_ports,
        scratchpad_count,
        ctx_stride,
    )?);

    xhci.halt()?;
    xhci.reset()?;

    let controller = Arc::new(usb::Controller {
        ops: Box::new(XhciControllerOps { ctrl: xhci.clone() }),
    });
    let root_hub = Hub::new_root(
        format!("usb-{address}"),
        controller,
        Box::new(XhciRootHubOps { ctrl: xhci.clone() }),
        max_ports,
    )?;
    *xhci.root_hub.lock() = Some(root_hub);

    let irq = view.setup_irq()?;
    xhci.start(irq)?;

    log!("Controller on {} initialized", address);
    CONTROLLERS.lock().push(xhci);
    Ok(())
}

fn handoff(mmio: &MmioView, xecp_dwords: u16) -> EResult<()> {
    if xecp_dwords == 0 {
        return Ok(());
    }

    let mut offset = xecp_dwords as usize * 4;
    loop {
        let reg = Register::<u32>::new(offset).with_le();
        let dword = unsafe { mmio.read_reg(reg) }.ok_or(Errno::EIO)?.value();

        let cap_id = dword & 0xFF;
        let next = (dword >> 8) & 0xFF;

        // USB Legacy Support capability.
        if cap_id == 1 {
            let bios_owned = (dword >> 16) & 0x1 != 0;
            if bios_owned {
                log!("Taking ownership from BIOS");
                // Set the OS Owned Semaphore (bit 24).
                unsafe { mmio.write_reg(reg, dword | (1 << 24)) };

                let deadline = clock::get_elapsed().saturating_add(HANDOFF_TIMEOUT_NS);
                loop {
                    let cur = unsafe { mmio.read_reg(reg) }.ok_or(Errno::EIO)?.value();
                    if (cur >> 16) & 0x1 == 0 {
                        break;
                    }
                    if clock::get_elapsed() >= deadline {
                        error!("BIOS ownership handoff timeout");
                        return Err(Errno::ETIMEDOUT);
                    }
                    let _ = clock::block_ns(10_000_000);
                }
            }
        }

        if next == 0 {
            break;
        }
        offset += next as usize * 4;
    }

    Ok(())
}

struct XhciController {
    mmio: Arc<MmioView>,
    opregs_base: usize,
    runtime_base: usize,
    doorbell_base: usize,
    slot_count: u8,
    port_count: u8,
    page_size: usize,
    ctx_stride: usize,

    command_ring: SpinMutex<Ring>,
    event_ring: SpinMutex<Ring>,

    slots: SpinMutex<Vec<Option<Arc<XhciDevice>>>>,
    root_hub: SpinMutex<Option<Arc<Hub>>>,

    // DMA structures kept alive for the controller's lifetime.
    dcbaa: OwnedPhysPages,
    erst: OwnedPhysPages,
    _scratchpad_array: Option<OwnedPhysPages>,
    _scratchpads: Vec<OwnedPhysPages>,

    irq: SpinMutex<Option<Arc<dyn IrqLine>>>,
}

impl XhciController {
    #[allow(clippy::too_many_arguments)]
    fn new(
        mmio: Arc<MmioView>,
        opregs_base: usize,
        runtime_base: usize,
        doorbell_base: usize,
        slot_count: u8,
        port_count: u8,
        scratchpad_count: u32,
        ctx_stride: usize,
    ) -> EResult<Self> {
        let page_size = arch::virt::get_page_size();

        let command_ring = Ring::new()?;
        let event_ring = Ring::new()?;

        // Device Context Base Address Array.
        let dcbaa = OwnedPhysPages::new(1, AllocFlags::empty())?;
        unsafe { core::ptr::write_bytes(dcbaa.as_hhdm::<u8>(), 0, page_size) };

        // Scratchpad buffers, if the controller requested any.
        let mut scratchpad_array = None;
        let mut scratchpads = Vec::new();
        if scratchpad_count > 0 {
            log!("Setting up {} scratchpad buffers", scratchpad_count);
            assert!((scratchpad_count as usize) < page_size / size_of::<u64>());

            let array = OwnedPhysPages::new(1, AllocFlags::empty())?;
            unsafe { core::ptr::write_bytes(array.as_hhdm::<u8>(), 0, page_size) };
            let array_entries = array.as_hhdm::<u64>();

            for i in 0..scratchpad_count as usize {
                let buf = OwnedPhysPages::new(1, AllocFlags::empty())?;
                unsafe { core::ptr::write_bytes(buf.as_hhdm::<u8>(), 0, page_size) };
                unsafe { array_entries.add(i).write(buf.phys().value() as u64) };
                scratchpads.push(buf);
            }

            // The scratchpad array pointer lives in DCBAA entry 0.
            unsafe { dcbaa.as_hhdm::<u64>().write(array.phys().value() as u64) };
            scratchpad_array = Some(array);
        }

        // Event Ring Segment Table with a single segment.
        let erst = OwnedPhysPages::new(1, AllocFlags::empty())?;
        unsafe { core::ptr::write_bytes(erst.as_hhdm::<u8>(), 0, page_size) };
        let erst_bytes =
            unsafe { core::slice::from_raw_parts_mut(erst.as_hhdm::<u8>(), erst_entry::SIZE) };
        erst_bytes.write_reg(
            erst_entry::RING_SEGMENT_BASE,
            event_ring.phys().value() as u64,
        );
        erst_bytes.write_reg(erst_entry::RING_SEGMENT_SIZE, event_ring.size as u16);

        let mut slots = Vec::new();
        slots.resize_with(slot_count as usize + 1, || None);

        Ok(Self {
            mmio,
            opregs_base,
            runtime_base,
            doorbell_base,
            slot_count,
            port_count,
            page_size,
            ctx_stride,
            command_ring: SpinMutex::new(command_ring),
            event_ring: SpinMutex::new(event_ring),
            slots: SpinMutex::new(slots),
            root_hub: SpinMutex::new(None),
            dcbaa,
            erst,
            _scratchpad_array: scratchpad_array,
            _scratchpads: scratchpads,
            irq: SpinMutex::new(None),
        })
    }

    fn opregs(&self) -> MmioSubView<'_> {
        self.mmio.sub_view(self.opregs_base).unwrap()
    }

    fn interrupter0(&self) -> MmioSubView<'_> {
        self.mmio
            .sub_view(self.runtime_base + runtime::INTERRUPTER_BASE)
            .unwrap()
    }

    fn port_regs(&self, index: usize) -> MmioSubView<'_> {
        self.mmio
            .sub_view(self.opregs_base + opregs::PORT_REGS_BASE + index * port::STRIDE)
            .unwrap()
    }

    fn op_rd(&self, reg: Register<u32>) -> BitValue<u32> {
        unsafe { self.opregs().read_reg(reg) }.unwrap()
    }

    fn op_wr(&self, reg: Register<u32>, value: u32) {
        unsafe { self.opregs().write_reg(reg, value) }.unwrap();
    }

    fn op_wr64(&self, reg: Register<u64>, value: u64) {
        unsafe { self.opregs().write_reg(reg, value) }.unwrap();
    }

    /// Stops the controller and waits for it to halt.
    fn halt(&self) -> EResult<()> {
        if self
            .op_rd(opregs::USBCMD)
            .read_field(opregs::usbcmd::RS)
            .value()
            == 0
        {
            return Ok(());
        }

        let cmd = self
            .op_rd(opregs::USBCMD)
            .write_field(opregs::usbcmd::RS, 0);
        self.op_wr(opregs::USBCMD, cmd.value());

        if !clock::poll_until(HANDSHAKE_TIMEOUT_NS, || {
            self.op_rd(opregs::USBSTS)
                .read_field(opregs::usbsts::HCH)
                .value()
                != 0
        }) {
            error!("controller halt timeout");
            return Err(Errno::ETIMEDOUT);
        }
        Ok(())
    }

    /// Resets the controller and waits for it to come back ready.
    fn reset(&self) -> EResult<()> {
        let cmd = self
            .op_rd(opregs::USBCMD)
            .write_field(opregs::usbcmd::HCRST, 1);
        self.op_wr(opregs::USBCMD, cmd.value());

        if !clock::poll_until(HANDSHAKE_TIMEOUT_NS, || {
            let cmd = self.op_rd(opregs::USBCMD);
            let sts = self.op_rd(opregs::USBSTS);
            cmd.read_field(opregs::usbcmd::HCRST).value() == 0
                && sts.read_field(opregs::usbsts::CNR).value() == 0
        }) {
            error!("Controller reset timeout");
            return Err(Errno::ETIMEDOUT);
        }
        Ok(())
    }

    /// Programs the controller's data structures and starts it.
    fn start(self: &Arc<Self>, irq: Arc<dyn IrqLine>) -> EResult<()> {
        // Enable the device slots.
        let config = BitValue::new(0u32).write_field(opregs::config::MAX_SLOTS_EN, self.slot_count);
        self.op_wr(opregs::CONFIG, config.value());

        // Point the controller at the Device Context Base Address Array.
        self.op_wr64(opregs::DCBAAP, self.dcbaa.phys().value() as u64);

        // Set up the command ring.
        let crcr = BitValue::new(self.command_ring.lock().phys().value() as u64)
            .write_field(opregs::crcr::RCS, 1);
        self.op_wr64(opregs::CRCR, crcr.value());

        // Set up the primary interrupter and event ring.
        let ir = self.interrupter0();
        let iman = BitValue::new(0u32).write_field(interrupter::iman::IE, 1);
        let event_phys = self.event_ring.lock().phys().value() as u64;
        unsafe {
            ir.write_reg(interrupter::IMAN, iman.value());
            ir.write_reg(interrupter::ERSTSZ, 1u32);
            ir.write_reg(interrupter::ERSTBA, self.erst.phys().value() as u64);
            let erdp = BitValue::new(event_phys).write_field(interrupter::erdp::EHB, 1);
            ir.write_reg(interrupter::ERDP, erdp.value());
        }

        // Hook up the interrupt before starting the controller.
        irq.attach(Box::new(XhciIrqHandler { ctrl: self.clone() }));
        irq.unmask();
        *self.irq.lock() = Some(irq);

        // Start the controller.
        let cmd = self
            .op_rd(opregs::USBCMD)
            .write_field(opregs::usbcmd::RS, 1)
            .write_field(opregs::usbcmd::INTE, 1);
        self.op_wr(opregs::USBCMD, cmd.value());

        if !clock::poll_until(HANDSHAKE_TIMEOUT_NS, || {
            let sts = self.op_rd(opregs::USBSTS);
            sts.read_field(opregs::usbsts::HCH).value() == 0
                && sts.read_field(opregs::usbsts::CNR).value() == 0
        }) {
            error!("Controller run timeout");
            return Err(Errno::ETIMEDOUT);
        }

        self.power_on_ports();
        Ok(())
    }

    fn power_on_ports(&self) {
        use port::portsc_bits::{CCS, CHANGE_BITS, PP, PR, PRESERVE_BITS};

        let mut debounce_wait = false;
        for i in 0..self.port_count as usize {
            let portsc = self.port_rd(i, port::PORTSC).value();
            // Acknowledge any latched change bits, preserving sticky state.
            self.port_wr(
                i,
                port::PORTSC,
                (portsc & PRESERVE_BITS) | (portsc & CHANGE_BITS),
            );
            if portsc & PP == 0 {
                self.port_wr(i, port::PORTSC, (portsc & CHANGE_BITS) | PP);
                debounce_wait = true;
            }
        }

        if debounce_wait {
            let _ = clock::block_ns(100_000_000); // 100ms debounce
        }

        for i in 0..self.port_count as usize {
            let portsc = self.port_rd(i, port::PORTSC).value();
            if portsc & CCS != 0 {
                self.port_wr(i, port::PORTSC, (portsc & PRESERVE_BITS) | PR);
            }
        }
    }

    pub(crate) fn port_rd(&self, index: usize, reg: Register<u32>) -> BitValue<u32> {
        unsafe { self.port_regs(index).read_reg(reg) }.unwrap()
    }

    fn port_wr(&self, index: usize, reg: Register<u32>, value: u32) {
        unsafe { self.port_regs(index).write_reg(reg, value) }.unwrap();
    }

    pub(crate) fn port_speed(&self, index: usize) -> u8 {
        self.port_rd(index, port::PORTSC)
            .read_field(port::portsc::SPEED)
            .value()
    }

    pub(crate) fn port_set_reset(&self, index: usize) {
        let portsc = self.port_rd(index, port::PORTSC).value();
        self.port_wr(
            index,
            port::PORTSC,
            (portsc & port::portsc_bits::PRESERVE_BITS) | port::portsc_bits::PR,
        );
    }

    pub(crate) fn port_set_power(&self, index: usize) {
        let portsc = self.port_rd(index, port::PORTSC).value();
        self.port_wr(
            index,
            port::PORTSC,
            (portsc & port::portsc_bits::CHANGE_BITS) | port::portsc_bits::PP,
        );
    }

    pub(crate) fn port_ack_change(&self, index: usize, bit: u32) {
        let portsc = self.port_rd(index, port::PORTSC).value();
        self.port_wr(
            index,
            port::PORTSC,
            (portsc & port::portsc_bits::PRESERVE_BITS) | bit,
        );
    }

    /// Rings a slot's doorbell with the given target.
    pub(crate) fn ring_doorbell(&self, slot: u8, target: u8) {
        let db = self
            .mmio
            .sub_view(self.doorbell_base + slot as usize * doorbell::STRIDE)
            .unwrap();
        let value = BitValue::new(0u32)
            .write_field(doorbell::TARGET, target)
            .value();
        unsafe { db.write_reg(doorbell::DOORBELL, value) }.unwrap();
    }

    fn ring_command_doorbell(&self) {
        self.ring_doorbell(0, 0);
    }

    /// Submits a command TRB and blocks until its completion event arrives.
    pub(crate) fn submit_command(&self, parameter: u64, control: u32) -> Completion {
        let cell = CompletionCell::new();
        {
            let mut ring = self.command_ring.lock();
            let index = ring.enqueue(parameter, 0, control);
            ring.set_pending(index, cell.clone());
        }
        self.ring_command_doorbell();
        cell.wait()
    }

    pub(crate) fn set_dcbaa(&self, slot: u8, phys: u64) {
        unsafe { self.dcbaa.as_hhdm::<u64>().add(slot as usize).write(phys) };
    }

    pub(crate) fn set_slot(&self, slot: u8, device: Arc<XhciDevice>) {
        if let Some(entry) = self.slots.lock().get_mut(slot as usize) {
            *entry = Some(device);
        }
    }

    /// Clears the DCBAA entry and slot-table entry for `slot`.
    pub(crate) fn release_slot(&self, slot: u8, device: &Arc<XhciDevice>) {
        self.set_dcbaa(slot, 0);
        if let Some(entry) = self.slots.lock().get_mut(slot as usize) {
            if entry.as_ref().is_some_and(|d| Arc::ptr_eq(d, device)) {
                *entry = None;
            }
        }
    }

    /// Drains the event ring, completing pending commands/transfers and recording port changes.
    fn process_events(&self) {
        let mut port_changes = [0u64; 4];

        {
            let ir = self.interrupter0();
            let mut event_ring = self.event_ring.lock();

            while let Some((parameter, status, control)) = event_ring.dequeue() {
                let trb_type = ((control >> 10) & 0x3f) as u8;
                let code = ((status >> 24) & 0xff) as u8;

                if trb_type == TrbType::CommandCompletionEvent as u8 {
                    let slot = ((control >> 24) & 0xff) as u32;
                    let mut command_ring = self.command_ring.lock();
                    if let Some(index) = command_ring.index_of_phys(parameter) {
                        if let Some(cell) = command_ring.take_pending(index) {
                            cell.complete(code, slot);
                        }
                    }
                } else if trb_type == TrbType::TransferEvent as u8 {
                    let residue = status & 0x00ff_ffff;
                    let slot = ((control >> 24) & 0xff) as usize;
                    let endpoint = ((control >> 16) & 0x1f) as usize;
                    let device = self.slots.lock().get(slot).and_then(|d| d.clone());
                    if let Some(device) = device {
                        if (1..=31).contains(&endpoint) {
                            let mut ring = device.ep_rings[endpoint - 1].lock();
                            if let Some(ring) = ring.as_mut() {
                                if let Some(index) = ring.index_of_phys(parameter) {
                                    if let Some(cell) = ring.take_pending(index) {
                                        cell.complete(code, residue);
                                    }
                                }
                            }
                        }
                    }
                } else if trb_type == TrbType::PortStatusChangeEvent as u8 {
                    let port_id = ((parameter >> 24) & 0xff) as usize;
                    if (1..=255).contains(&port_id) {
                        port_changes[(port_id - 1) / 64] |= 1 << ((port_id - 1) % 64);
                    }
                }
            }

            // Advance the event ring dequeue pointer and clear the busy bit.
            let phys = event_ring.trb_phys(event_ring.index).value() as u64;
            let erdp = BitValue::new(phys)
                .write_field(interrupter::erdp::EHB, 1)
                .value();
            unsafe { ir.write_reg(interrupter::ERDP, erdp) };
        }

        // Handle port changes outside the event ring lock.
        for (word, mut bits) in port_changes.into_iter().enumerate() {
            while bits != 0 {
                let bit = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                self.handle_port_change((word * 64 + bit + 1) as u8);
            }
        }
    }

    /// Decodes a port's PORTSC and notifies the root hub.
    fn handle_port_change(&self, port_id: u8) {
        if port_id == 0 || port_id > self.port_count {
            return;
        }
        let index = (port_id - 1) as usize;

        let hub = self.root_hub.lock().clone();
        let Some(hub) = hub else {
            return;
        };

        let portsc = self.port_rd(index, port::PORTSC).value();

        if portsc & port::portsc_bits::CSC != 0 {
            self.port_ack_change(index, port::portsc_bits::CSC);
            if portsc & port::portsc_bits::CCS != 0 {
                hub.handle_connect(index as u8);
            } else {
                hub.handle_disconnect(index as u8);
            }
        }

        if portsc & port::portsc_bits::PRC != 0 {
            self.port_ack_change(index, port::portsc_bits::PRC);
            if portsc & port::portsc_bits::PED != 0 {
                hub.handle_reset(index as u8, xhci_speed_to_usb(self.port_speed(index)));
            }
        }
    }
}

struct XhciIrqHandler {
    ctrl: Arc<XhciController>,
}

impl IrqHandler for XhciIrqHandler {
    fn raise(&mut self) -> Status {
        let op = self.ctrl.opregs();
        let ir = self.ctrl.interrupter0();

        let Some(iman) = (unsafe { ir.read_reg(interrupter::IMAN) }) else {
            return Status::Ignored;
        };

        // Clear EINT then IMAN.IP to acknowledge.
        let clear_eint = BitValue::new(0u32).write_field(opregs::usbsts::EINT, 1);
        unsafe {
            op.write_reg(opregs::USBSTS, clear_eint.value());
            ir.write_reg(interrupter::IMAN, iman.value());
        }

        self.ctrl.process_events();

        Status::Handled
    }
}
