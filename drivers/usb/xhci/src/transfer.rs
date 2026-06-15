use crate::{
    XhciController,
    device::XhciDevice,
    ring::CompletionCell,
    spec::{CompletionCode, TrbType, trb},
};
use zinnia::{
    arch,
    device::usb::{Status, Transfer, TransferFlags, UsbResult},
    memory::{AllocFlags, BitValue, OwnedPhysPages},
};

pub(crate) fn status_from_code(code: u8) -> UsbResult<()> {
    if code == CompletionCode::Success as u8 || code == CompletionCode::ShortPacket as u8 {
        Ok(())
    } else if code == CompletionCode::Stall as u8 {
        Err(Status::Stall)
    } else {
        Err(Status::Error)
    }
}

fn stage_data(
    xfer: &mut Transfer,
    length: usize,
    to_host: bool,
) -> UsbResult<(OwnedPhysPages, u64)> {
    let page_size = arch::virt::get_page_size();
    let pages = OwnedPhysPages::new(length.div_ceil(page_size), AllocFlags::empty())
        .map_err(|_| Status::Error)?;

    if !to_host {
        let slice = unsafe { core::slice::from_raw_parts_mut(pages.as_hhdm::<u8>(), length) };
        xfer.data.copy_to_slice(slice).map_err(|_| Status::Error)?;
    }

    let phys = pages.phys().value() as u64;
    Ok((pages, phys))
}

/// Copies a completed IN transfer's data back into the iovec.
fn unstage_data(xfer: &mut Transfer, bounce: &OwnedPhysPages, transferred: usize) -> UsbResult<()> {
    let slice = unsafe { core::slice::from_raw_parts(bounce.as_hhdm::<u8>(), transferred) };
    xfer.data
        .copy_from_slice(slice)
        .map_err(|_| Status::Error)?;
    Ok(())
}

/// Submits a control transfer on EP0.
pub fn control(ctrl: &XhciController, dev: &XhciDevice, xfer: &mut Transfer) -> UsbResult<usize> {
    let setup = xfer.setup.ok_or(Status::Error)?;
    let length = setup.length as usize;
    let to_host = xfer.flags.contains(TransferFlags::ToHost);
    let has_data = length > 0;

    let mut bounce = None;
    let mut data_phys = 0u64;
    if has_data {
        let (pages, phys) = stage_data(xfer, length, to_host)?;
        data_phys = phys;
        bounce = Some(pages);
    }

    let cell = CompletionCell::new();
    {
        let mut ring = dev.ep_rings[0].lock();
        let ring = ring.as_mut().ok_or(Status::Error)?;

        // Setup stage.
        let trt = if !has_data {
            0u8
        } else if to_host {
            3
        } else {
            2
        };
        let setup_param = (setup.request_type as u64)
            | ((setup.request as u64) << 8)
            | ((setup.value as u64) << 16)
            | ((setup.index as u64) << 32)
            | ((setup.length as u64) << 48);
        let setup_control = BitValue::new(0u32)
            .write_field(trb::control::TRB_TYPE, TrbType::SetupStage as u8)
            .write_field(trb::control::TRT, trt)
            .write_field(trb::control::IDT, 1)
            .value();
        ring.enqueue(setup_param, 8, setup_control);

        // Data stage.
        if has_data {
            let data_control = BitValue::new(0u32)
                .write_field(trb::control::TRB_TYPE, TrbType::DataStage as u8)
                .write_field(trb::control::ISP, 1)
                .write_field(trb::control::DIR, to_host as u8)
                .value();
            let status = BitValue::new(0u32)
                .write_field(trb::status::TRANSFER_LEN, length as u32)
                .value();
            let idx = ring.enqueue(data_phys, status, data_control);
            ring.set_pending(idx, cell.clone());
        }

        // Status stage.
        let status_control = BitValue::new(0u32)
            .write_field(trb::control::TRB_TYPE, TrbType::StatusStage as u8)
            .write_field(trb::control::IOC, 1)
            .write_field(trb::control::DIR, (!to_host) as u8)
            .value();
        let idx = ring.enqueue(0, 0, status_control);
        ring.set_pending(idx, cell.clone());
    }

    // Doorbell target 1 = EP0.
    ctrl.ring_doorbell(dev.slot_id(), 1);

    let completion = cell.wait();
    status_from_code(completion.code)?;
    let transferred = length.saturating_sub(completion.value as usize);

    if has_data && to_host {
        unstage_data(xfer, bounce.as_ref().unwrap(), transferred)?;
    }

    Ok(transferred)
}

/// Submits a bulk or interrupt transfer.
pub fn data(ctrl: &XhciController, dev: &XhciDevice, xfer: &mut Transfer) -> UsbResult<usize> {
    let endpoint = xfer.endpoint.ok_or(Status::Error)?;
    let address = endpoint.desc.endpoint_address;
    let ep_num = (address & 0x0f) as usize;
    let dir_in = address & 0x80 != 0;
    let ep_index = (ep_num << 1) | dir_in as usize;
    if ep_index == 0 || ep_index > 31 {
        return Err(Status::Error);
    }

    let length = xfer.data.len();
    if length == 0 {
        return Ok(0);
    }

    let (bounce, data_phys) = stage_data(xfer, length, dir_in)?;

    let cell = CompletionCell::new();
    {
        let mut ring = dev.ep_rings[ep_index - 1].lock();
        let ring = ring.as_mut().ok_or(Status::Error)?;
        let control = BitValue::new(0u32)
            .write_field(trb::control::TRB_TYPE, TrbType::Normal as u8)
            .write_field(trb::control::ISP, 1)
            .write_field(trb::control::IOC, 1)
            .value();
        let status = BitValue::new(0u32)
            .write_field(trb::status::TRANSFER_LEN, length as u32)
            .value();
        let idx = ring.enqueue(data_phys, status, control);
        ring.set_pending(idx, cell.clone());
    }

    ctrl.ring_doorbell(dev.slot_id(), ep_index as u8);

    let completion = cell.wait();
    status_from_code(completion.code)?;
    let transferred = length.saturating_sub(completion.value as usize);

    if dir_in {
        unstage_data(xfer, &bounce, transferred)?;
    }

    Ok(transferred)
}
