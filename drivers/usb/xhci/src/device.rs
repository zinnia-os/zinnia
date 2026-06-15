use crate::{
    XhciController,
    ring::Ring,
    spec::{CONTEXT_SIZE, EndpointType, TrbType, ep_ctx, input_ctx, slot_ctx, trb},
    transfer::{self, status_from_code},
};
use core::sync::atomic::{AtomicU8, Ordering};
use zinnia::{
    alloc::boxed::Box,
    alloc::sync::Arc,
    async_trait::async_trait,
    device::usb::{
        Controller, ControllerOps, Device, Endpoint, Speed, Status, Transfer, TransferType,
        UsbResult,
        hub::Hub,
        spec::{DescriptorType, DeviceDescriptor},
    },
    log,
    memory::{AllocFlags, BitValue, MemoryView, OwnedPhysPages},
    util::mutex::spin::SpinMutex,
    warn,
};

/// Per device xHCI state.
pub struct XhciDevice {
    slot_id: AtomicU8,
    ctx_stride: usize,
    speed: Speed,
    /// One 4-bit hub port number per tier below the root.
    route_string: u32,
    /// The 1-based root-hub port this device's topology hangs off.
    root_port: u8,
    /// Hub nesting depth.
    tier: u8,
    hub_fields: SpinMutex<Option<(u8, bool)>>,
    device_ctx: OwnedPhysPages,
    input_ctx: OwnedPhysPages,
    /// One transfer ring per endpoint, indexed by `dci - 1` (EP0 is dci 1).
    pub ep_rings: [SpinMutex<Option<Ring>>; 31],
}

impl XhciDevice {
    fn new(
        ctx_stride: usize,
        speed: Speed,
        route_string: u32,
        root_port: u8,
        tier: u8,
        device_ctx: OwnedPhysPages,
        input_ctx: OwnedPhysPages,
    ) -> Self {
        Self {
            slot_id: AtomicU8::new(0),
            ctx_stride,
            speed,
            route_string,
            root_port,
            tier,
            hub_fields: SpinMutex::new(None),
            device_ctx,
            input_ctx,
            ep_rings: core::array::from_fn(|_| SpinMutex::new(None)),
        }
    }

    pub fn slot_id(&self) -> u8 {
        self.slot_id.load(Ordering::Acquire)
    }

    fn device_ctx_phys(&self) -> u64 {
        self.device_ctx.phys().value() as u64
    }

    fn input_ctx_phys(&self) -> u64 {
        self.input_ctx.phys().value() as u64
    }

    #[allow(clippy::mut_from_ref)]
    fn input_ctx_view(&self, index: usize) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.input_ctx.as_hhdm::<u8>().add(index * self.ctx_stride),
                CONTEXT_SIZE,
            )
        }
    }

    fn update_input_context(&self) {
        let stride = self.ctx_stride;
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.device_ctx.as_hhdm::<u8>(),
                self.input_ctx.as_hhdm::<u8>().add(stride),
                stride * 32,
            );
            core::ptr::write_bytes(self.input_ctx.as_hhdm::<u8>(), 0, 8);
        }
    }
}

fn xdev_of(device: &Device) -> UsbResult<Arc<XhciDevice>> {
    let data = device.driver_data.lock();
    let any = data.as_ref().ok_or(Status::Error)?.clone();
    any.downcast::<XhciDevice>().map_err(|_| Status::Error)
}

pub(crate) fn xhci_speed_to_usb(code: u8) -> Speed {
    match code {
        1 => Speed::Full,
        2 => Speed::Low,
        3 => Speed::High,
        4 => Speed::Super,
        5 | 6 | 7 => Speed::SuperPlus,
        _ => Speed::Unknown,
    }
}

fn usb_speed_to_xhci(speed: Speed) -> u8 {
    match speed {
        Speed::Full => 1,
        Speed::Low => 2,
        Speed::High => 3,
        Speed::Super | Speed::SuperPlus | Speed::Unknown => 4,
    }
}

/// Default EP0 max packet size implied by the device speed.
fn ep0_max_packet_size(ctrl_speed: u8) -> u16 {
    match ctrl_speed {
        1 | 2 => 8, // Full / Low
        3 => 64,    // High
        _ => 512,   // Super and above
    }
}

fn ceil_log2(value: u32) -> u32 {
    if value <= 1 {
        0
    } else {
        32 - (value - 1).leading_zeros()
    }
}

pub struct XhciControllerOps {
    pub ctrl: Arc<XhciController>,
}

#[async_trait(?Send)]
impl ControllerOps for XhciControllerOps {
    async fn address_device(
        &self,
        _controller: &Controller,
        hub: &Arc<Hub>,
        port: u8,
        speed: Speed,
    ) -> UsbResult<Arc<Device>> {
        let ctrl = &self.ctrl;
        let stride = ctrl.ctx_stride;

        let device_ctx = OwnedPhysPages::new(1, AllocFlags::empty()).map_err(|_| Status::Error)?;
        let input_ctx = OwnedPhysPages::new(1, AllocFlags::empty()).map_err(|_| Status::Error)?;
        unsafe {
            core::ptr::write_bytes(device_ctx.as_hhdm::<u8>(), 0, ctrl.page_size);
            core::ptr::write_bytes(input_ctx.as_hhdm::<u8>(), 0, ctrl.page_size);
        }

        let ep0_ring = Ring::new().map_err(|_| Status::Error)?;
        let ep0_phys = ep0_ring.phys().value() as u64;
        let ep0_cycle = ep0_ring.cycle;

        let parent = match &hub.device {
            Some(hub_device) => Some(xdev_of(hub_device)?),
            None => None,
        };
        let (ctrl_speed, usb_speed, route_string, root_port, tier) = match &parent {
            Some(parent) => {
                let route_string =
                    parent.route_string | ((port as u32 + 1).min(15) << (parent.tier as u32 * 4));
                (
                    usb_speed_to_xhci(speed),
                    speed,
                    route_string,
                    parent.root_port,
                    parent.tier + 1,
                )
            }
            None => {
                let ctrl_speed = ctrl.port_speed(port as usize);
                (ctrl_speed, xhci_speed_to_usb(ctrl_speed), 0, port + 1, 0)
            }
        };

        let xdev = Arc::new(XhciDevice::new(
            stride,
            usb_speed,
            route_string,
            root_port,
            tier,
            device_ctx,
            input_ctx,
        ));
        *xdev.ep_rings[0].lock() = Some(ep0_ring);

        // Enable Slot.
        let control = BitValue::new(0u32)
            .write_field(trb::control::TRB_TYPE, TrbType::EnableSlot as u8)
            .value();
        let completion = ctrl.submit_command(0, control);
        status_from_code(completion.code)?;
        let slot_id = completion.value as u8;
        xdev.slot_id.store(slot_id, Ordering::Release);

        ctrl.set_dcbaa(slot_id, xdev.device_ctx_phys());
        ctrl.set_slot(slot_id, xdev.clone());

        // Build the input context for Address Device, enable slot + EP0.
        {
            let cc = xdev.input_ctx_view(0);
            cc.write_reg(input_ctx::DROP_FLAGS, 0u32);
            cc.write_reg(input_ctx::ADD_FLAGS, 0b11u32);

            let slot = xdev.input_ctx_view(1);
            let dw0 = BitValue::new(0u32)
                .write_field(slot_ctx::ROUTE_STRING, route_string)
                .write_field(slot_ctx::SPEED, ctrl_speed)
                .write_field(slot_ctx::CTX_ENTRIES, 1u8)
                .value();
            slot.write_reg(slot_ctx::DW0, dw0);
            let dw1 = BitValue::new(0u32)
                .write_field(slot_ctx::ROOT_HUB_PORT_NUMBER, root_port)
                .value();
            slot.write_reg(slot_ctx::DW1, dw1);

            // Low/full-speed devices behind a high-speed hub go through that hub's transaction translator.
            if let Some(parent) = &parent
                && matches!(usb_speed, Speed::Low | Speed::Full)
                && hub.device.as_ref().is_some_and(|d| d.speed == Speed::High)
            {
                let dw2 = BitValue::new(0u32)
                    .write_field(slot_ctx::TT_HUB_SLOT_ID, parent.slot_id())
                    .write_field(slot_ctx::TT_PORT_NUMBER, port + 1)
                    .write_field(slot_ctx::TTT, 0u8)
                    .value();
                slot.write_reg(slot_ctx::DW2, dw2);
            }

            let ep0 = xdev.input_ctx_view(2);
            let dw1 = BitValue::new(0u32)
                .write_field(ep_ctx::EP_TYPE, EndpointType::Control as u8)
                .write_field(ep_ctx::CERR, 3u8)
                .write_field(ep_ctx::MAX_PACKET_SIZE, ep0_max_packet_size(ctrl_speed))
                .value();
            ep0.write_reg(ep_ctx::DW1, dw1);
            ep0.write_reg(ep_ctx::TR_DEQUEUE_PTR, ep0_phys | ep0_cycle as u64);
        }

        let control = BitValue::new(0u32)
            .write_field(trb::control::TRB_TYPE, TrbType::AddressDevice as u8)
            .write_field(trb::control::SLOT_ID, slot_id)
            .value();
        let completion = ctrl.submit_command(xdev.input_ctx_phys(), control);
        if let Err(e) = status_from_code(completion.code) {
            ctrl.release_slot(slot_id, &xdev);
            return Err(e);
        }
        log!("Addressed device on slot {}", slot_id);

        // Now that EP0 works, create the device.
        let device = Arc::new(Device::new(hub.clone(), port, usb_speed));
        *device.driver_data.lock() = Some(xdev.clone());

        let mut head = [0u8; 8];
        let transferred = device
            .get_descriptor(DescriptorType::Device as u8, 0, &mut head)
            .await?;
        if transferred < head.len() || head[1] != DescriptorType::Device as u8 {
            warn!(
                "Slot {} short/invalid device descriptor header ({} of 8 bytes, type {:#x})",
                slot_id, transferred, head[1]
            );
            ctrl.release_slot(slot_id, &xdev);
            return Err(Status::Error);
        }
        let mps0 = head[7];
        let real_mps = if matches!(usb_speed, Speed::Super | Speed::SuperPlus) {
            1u16 << mps0
        } else {
            mps0 as u16
        };
        {
            let cc = xdev.input_ctx_view(0);
            cc.write_reg(input_ctx::DROP_FLAGS, 0u32);
            cc.write_reg(input_ctx::ADD_FLAGS, 0b10u32);

            let ep0 = xdev.input_ctx_view(2);
            let dw1 = ep0
                .read_reg(ep_ctx::DW1)
                .unwrap()
                .write_field(ep_ctx::MAX_PACKET_SIZE, real_mps);
            ep0.write_reg(ep_ctx::DW1, dw1.value());
        }
        let control = BitValue::new(0u32)
            .write_field(trb::control::TRB_TYPE, TrbType::EvaluateContext as u8)
            .write_field(trb::control::SLOT_ID, slot_id)
            .value();
        let completion = ctrl.submit_command(xdev.input_ctx_phys(), control);
        if let Err(e) = status_from_code(completion.code) {
            ctrl.release_slot(slot_id, &xdev);
            return Err(e);
        }

        // With the correct max packet size in place, read the full descriptor.
        let mut desc = [0u8; size_of::<DeviceDescriptor>()];
        let transferred = device
            .get_descriptor(DescriptorType::Device as u8, 0, &mut desc)
            .await?;
        if transferred < desc.len() || desc[1] != DescriptorType::Device as u8 {
            warn!(
                "Slot {} short/invalid device descriptor ({} of {} bytes, type {:#x})",
                slot_id,
                transferred,
                desc.len(),
                desc[1]
            );
            ctrl.release_slot(slot_id, &xdev);
            return Err(Status::Error);
        }

        let descriptor =
            unsafe { core::ptr::read_unaligned(desc.as_ptr() as *const DeviceDescriptor) };
        if !descriptor.is_valid(usb_speed, transferred) {
            warn!(
                "Slot {} device descriptor failed validation (speed {:?}, mps0 {})",
                slot_id, usb_speed, descriptor.max_packet_size_ep0
            );
            ctrl.release_slot(slot_id, &xdev);
            return Err(Status::Error);
        }
        *device.descriptor.lock() = Some(descriptor);

        Ok(device)
    }

    async fn deaddress_device(&self, _controller: &Controller, device: &Device) -> UsbResult<()> {
        let xdev = xdev_of(device)?;
        let slot = xdev.slot_id();

        let control = BitValue::new(0u32)
            .write_field(trb::control::TRB_TYPE, TrbType::DisableSlot as u8)
            .write_field(trb::control::SLOT_ID, slot)
            .value();
        let completion = self.ctrl.submit_command(0, control);
        self.ctrl.release_slot(slot, &xdev);
        status_from_code(completion.code)
    }

    async fn mark_as_hub(&self, _controller: &Controller, device: &Device) -> UsbResult<()> {
        let info = (*device.hub_info.lock()).ok_or(Status::Error)?;
        let xdev = xdev_of(device)?;

        *xdev.hub_fields.lock() = Some((info.port_count, info.multi_tt));
        Ok(())
    }

    async fn configure_ep(
        &self,
        _controller: &Controller,
        device: &Device,
        endpoint: &Endpoint,
    ) -> UsbResult<()> {
        let ctrl = &self.ctrl;
        let xdev = xdev_of(device)?;

        let address = endpoint.desc.endpoint_address;
        let ep_num = (address & 0x0f) as usize;
        let dir_in = address & 0x80 != 0;
        let ep_index = (ep_num << 1) | dir_in as usize;
        if ep_index == 0 || ep_index > 31 {
            return Err(Status::Error);
        }
        if xdev.ep_rings[ep_index - 1].lock().is_some() {
            return Err(Status::Error);
        }

        let ring = Ring::new().map_err(|_| Status::Error)?;
        let ring_phys = ring.phys().value() as u64;
        let ring_cycle = ring.cycle;
        *xdev.ep_rings[ep_index - 1].lock() = Some(ring);

        xdev.update_input_context();

        {
            let cc = xdev.input_ctx_view(0);
            cc.write_reg(input_ctx::DROP_FLAGS, 0u32);
            cc.write_reg(input_ctx::ADD_FLAGS, (1u32 << 0) | (1u32 << ep_index));
        }
        {
            let slot = xdev.input_ctx_view(1);
            let dw0 = slot.read_reg(slot_ctx::DW0).unwrap();
            if (dw0.read_field(slot_ctx::CTX_ENTRIES).value() as usize) < ep_index {
                let dw0 = dw0.write_field(slot_ctx::CTX_ENTRIES, ep_index as u8);
                slot.write_reg(slot_ctx::DW0, dw0.value());
            }

            if let Some((port_count, multi_tt)) = *xdev.hub_fields.lock() {
                let dw0 = slot
                    .read_reg(slot_ctx::DW0)
                    .unwrap()
                    .write_field(slot_ctx::HUB, 1u8)
                    .write_field(slot_ctx::MTT, multi_tt as u8);
                slot.write_reg(slot_ctx::DW0, dw0.value());
                let dw1 = slot
                    .read_reg(slot_ctx::DW1)
                    .unwrap()
                    .write_field(slot_ctx::NUMBER_OF_PORTS, port_count);
                slot.write_reg(slot_ctx::DW1, dw1.value());
            }
        }

        let attributes = endpoint.desc.attributes;
        let mps = endpoint.desc.max_packet_size();
        let interval_raw = endpoint.desc.interval;
        let (max_burst, max_esit_payload) = match endpoint.ss_companion {
            Some(companion) if matches!(xdev.speed, Speed::Super | Speed::SuperPlus) => {
                (companion.max_burst, companion.bytes_per_interval)
            }
            // High-speed periodic endpoints may use multiple transactions per microframe.
            _ if xdev.speed == Speed::High => (0, mps * (1 + endpoint.desc.extra_transactions())),
            _ => (0, mps),
        };

        let ep_type = match (attributes & 0x03, dir_in) {
            (2, false) => EndpointType::BulkOut,
            (2, true) => EndpointType::BulkIn,
            (3, false) => EndpointType::InterruptOut,
            (3, true) => EndpointType::InterruptIn,
            _ => {
                *xdev.ep_rings[ep_index - 1].lock() = None;
                return Err(Status::Error);
            }
        };
        let interval = if attributes & 0x03 == 3 {
            match xdev.speed {
                Speed::Low | Speed::Full => ceil_log2(interval_raw as u32 * 8) as u8,
                _ => interval_raw.saturating_sub(1),
            }
        } else {
            0
        };

        {
            let ep = xdev.input_ctx_view(ep_index + 1);
            let dw0 = BitValue::new(0u32)
                .write_field(ep_ctx::INTERVAL, interval)
                .value();
            ep.write_reg(ep_ctx::DW0, dw0);
            let dw1 = BitValue::new(0u32)
                .write_field(ep_ctx::EP_TYPE, ep_type as u8)
                .write_field(ep_ctx::CERR, 3u8)
                .write_field(ep_ctx::MAX_BURST_SIZE, max_burst)
                .write_field(ep_ctx::MAX_PACKET_SIZE, mps)
                .value();
            ep.write_reg(ep_ctx::DW1, dw1);
            ep.write_reg(ep_ctx::TR_DEQUEUE_PTR, ring_phys | ring_cycle as u64);

            // The controller schedules periodic bandwidth from the largest payload per service interval.
            if attributes & 0x03 == 3 {
                let dw4 = BitValue::new(0u32)
                    .write_field(ep_ctx::MAX_ESIT_PAYLOAD_LO, max_esit_payload)
                    .value();
                ep.write_reg(ep_ctx::DW4, dw4);
            }
        }

        let control = BitValue::new(0u32)
            .write_field(trb::control::TRB_TYPE, TrbType::ConfigureEndpoint as u8)
            .write_field(trb::control::SLOT_ID, xdev.slot_id())
            .value();
        let completion = ctrl.submit_command(xdev.input_ctx_phys(), control);
        if let Err(e) = status_from_code(completion.code) {
            *xdev.ep_rings[ep_index - 1].lock() = None;
            return Err(e);
        }
        Ok(())
    }

    async fn transfer(
        &self,
        _controller: &Controller,
        device: &Device,
        xfer: &mut Transfer,
    ) -> UsbResult<usize> {
        let xdev = xdev_of(device)?;
        match xfer.typ {
            TransferType::Control => transfer::control(&self.ctrl, &xdev, xfer),
            TransferType::Bulk | TransferType::Interrupt => transfer::data(&self.ctrl, &xdev, xfer),
        }
    }
}
