use super::{Device as UsbDevice, Driver, Endpoint, Interface, Status, spec};
use crate::{
    device::{
        Device,
        block::{BlockCompletion, BlockDevice, BlockIo, BlockOp, register_block_device},
    },
    memory::PhysAddr,
    percpu::CpuData,
    posix::errno::{EResult, Errno},
    process::task::Task,
    util::mutex::Mutex,
    vfs::file::{FileOps, OpenFlags},
};
use alloc::{format, sync::Arc};
use core::sync::atomic::{AtomicU32, Ordering};

#[initgraph::task(name = "device.usb.storage", depends = [crate::memory::MEMORY_STAGE])]
pub fn STORAGE_STAGE() {
    DRIVER.register();
}

static DRIVER: Driver = Driver {
    name: "usb-storage",
    probe,
    attach,
    detach,
};

const MSC_CLASS: u8 = 0x08;
const MSC_SUBCLASS_SCSI: u8 = 0x06;
const MSC_PROTOCOL_BBB: u8 = 0x50;

const CBW_SIGNATURE: u32 = 0x4342_5355;
const CSW_SIGNATURE: u32 = 0x5342_5355;
const CBW_FLAG_DATA_IN: u8 = 0x80;

const USB_REQUEST_CLEAR_FEATURE: u8 = 1;
const USB_FEATURE_ENDPOINT_HALT: u16 = 0;
/// Bulk-only class request.
const MSC_REQUEST_GET_MAX_LUN: u8 = 0xfe;

const SCSI_TEST_UNIT_READY: u8 = 0x00;
const SCSI_REQUEST_SENSE: u8 = 0x03;
const SCSI_INQUIRY: u8 = 0x12;
const SCSI_READ_CAPACITY_10: u8 = 0x25;
const SCSI_READ_10: u8 = 0x28;
const SCSI_WRITE_10: u8 = 0x2a;

/// A single bulk TRB carries at most 64 KiB, so cap each BOT data phase to that.
const MAX_BOT_BYTES: usize = 64 * 1024;
const MSC_MAJOR: u32 = 188;

static TAG: AtomicU32 = AtomicU32::new(1);
static MINOR: AtomicU32 = AtomicU32::new(0);

fn next_tag() -> u32 {
    TAG.fetch_add(1, Ordering::Relaxed)
}

fn probe(_device: Arc<UsbDevice>, interface: &Interface) -> EResult<()> {
    if interface.desc.interface_class == MSC_CLASS
        && interface.desc.interface_sub_class == MSC_SUBCLASS_SCSI
        && interface.desc.interface_protocol == MSC_PROTOCOL_BBB
    {
        Ok(())
    } else {
        Err(Errno::ENODEV)
    }
}

fn attach(device: Arc<UsbDevice>, interface: &Interface) -> EResult<()> {
    let mut in_desc = None;
    let mut out_desc = None;
    for ep in &interface.endpoints {
        if ep.desc.attributes & 0x03 != 2 {
            continue;
        }
        if ep.desc.endpoint_address & 0x80 != 0 {
            in_desc = Some(ep.desc);
        } else {
            out_desc = Some(ep.desc);
        }
    }
    let (Some(in_desc), Some(out_desc)) = (in_desc, out_desc) else {
        warn!("Mass storage interface missing bulk endpoints");
        return Err(Errno::ENODEV);
    };
    let interface_number = interface.desc.interface_number;

    Task::run(move |_| {
        let bulk_in = Endpoint {
            desc: in_desc,
            ss_companion: None,
        };
        let bulk_out = Endpoint {
            desc: out_desc,
            ss_companion: None,
        };

        log!(
            "USB storage: interface {}, speed {:?}, bulk in {:#04x} out {:#04x}",
            interface_number,
            device.speed,
            { in_desc.endpoint_address },
            { out_desc.endpoint_address },
        );

        let Some((lba_size, lba_count)) = CpuData::get().scheduler.block_on(probe_geometry(
            &device,
            &bulk_in,
            &bulk_out,
            interface_number,
        )) else {
            return;
        };

        let minor = MINOR.fetch_add(1, Ordering::Relaxed);
        let dev = Arc::new(MassStorage {
            device,
            bulk_in,
            bulk_out,
            lba_size,
            lba_count,
            bot: Mutex::new(()),
            minor,
        });

        let name = format!("usb{minor}");
        if let Err(e) = register_block_device(&name, dev) {
            warn!("Failed to register USB mass storage device: {:?}", e);
        }
    })?;

    Ok(())
}

fn detach(_device: Arc<UsbDevice>, _interface: &Interface) -> EResult<()> {
    // TODO: signal the worker to stop and unregister the block device.
    Ok(())
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct CommandBlockWrapper {
    signature: u32,
    tag: u32,
    data_transfer_length: u32,
    flags: u8,
    lun: u8,
    cdb_length: u8,
    cdb: [u8; 16],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct CommandStatusWrapper {
    signature: u32,
    tag: u32,
    data_residue: u32,
    status: u8,
}

/// Data phase of a BOT command.
enum Data<'a> {
    None,
    /// A kernel slice for small commands.
    Buf(&'a mut [u8], bool),
    /// A contiguous physical buffer DMA'd directly.
    Phys(PhysAddr, usize, bool),
}

/// Runs one Bulk-Only Transport command
async fn bot_transfer(
    device: &UsbDevice,
    bulk_in: &Endpoint,
    bulk_out: &Endpoint,
    tag: u32,
    cdb: &[u8],
    data: Data<'_>,
) -> EResult<()> {
    if cdb.len() > 16 {
        return Err(Errno::EINVAL);
    }

    let (data_len, to_host) = match &data {
        Data::None => (0, false),
        Data::Buf(buf, th) => (buf.len(), *th),
        Data::Phys(_, len, th) => (*len, *th),
    };

    let mut cbw = CommandBlockWrapper {
        signature: CBW_SIGNATURE,
        tag,
        data_transfer_length: data_len as u32,
        flags: if to_host { CBW_FLAG_DATA_IN } else { 0 },
        lun: 0,
        cdb_length: cdb.len() as u8,
        cdb: [0; 16],
    };
    cbw.cdb[..cdb.len()].copy_from_slice(cdb);

    let mut cbw_bytes = [0u8; size_of::<CommandBlockWrapper>()];
    let len = cbw_bytes.len();
    cbw_bytes.copy_from_slice(unsafe {
        core::slice::from_raw_parts(&cbw as *const _ as *const u8, len)
    });
    device
        .bulk_transfer(bulk_out, &mut cbw_bytes, false)
        .await
        .map_err(|_| Errno::EIO)?;

    let data_ep = if to_host { bulk_in } else { bulk_out };
    let data_result = match data {
        Data::None => None,
        Data::Buf(buf, th) => Some(device.bulk_transfer(data_ep, buf, th).await),
        Data::Phys(phys, len, th) => Some(device.bulk_transfer_phys(data_ep, phys, len, th).await),
    };
    if let Some(result) = data_result {
        match result {
            Ok(_) => {}
            // A stalled data phase is recoverable: clear the halt and read the CSW.
            Err(Status::Stall) => {
                let _ = clear_halt(device, data_ep.desc.endpoint_address).await;
            }
            Err(_) => return Err(Errno::EIO),
        }
    }

    // Read the CSW.
    let mut csw_bytes = [0u8; size_of::<CommandStatusWrapper>()];
    let mut got_csw = false;
    for attempt in 0..2 {
        match device.bulk_transfer(bulk_in, &mut csw_bytes, true).await {
            Ok(_) => {
                got_csw = true;
                break;
            }
            Err(Status::Stall) if attempt == 0 => {
                let _ = clear_halt(device, bulk_in.desc.endpoint_address).await;
            }
            Err(_) => break,
        }
    }
    if !got_csw {
        return Err(Errno::EIO);
    }

    let csw = unsafe { &*(csw_bytes.as_ptr() as *const CommandStatusWrapper) };
    if { csw.signature } != CSW_SIGNATURE || { csw.tag } != tag || csw.status != 0 {
        return Err(Errno::EIO);
    }

    Ok(())
}

/// Clears an endpoint halt on the control endpoint.
async fn clear_halt(device: &UsbDevice, ep_address: u8) -> Result<usize, Status> {
    let setup = spec::Setup {
        request_type: (spec::USB_REQUEST_DIR_TO_DEVICE
            | spec::USB_REQUEST_STANDARD
            | spec::USB_REQUEST_RECIP_ENDPOINT) as u8,
        request: USB_REQUEST_CLEAR_FEATURE,
        value: USB_FEATURE_ENDPOINT_HALT,
        index: ep_address as u16,
        length: 0,
    };
    device.control_transfer(setup, &mut []).await
}

/// Returns the device's maximum LUN, or 0 if it does not implement the request.
async fn get_max_lun(device: &UsbDevice, interface: u16) -> u8 {
    let mut buf = [0u8; 1];
    let setup = spec::Setup {
        request_type: (spec::USB_REQUEST_DIR_TO_HOST
            | spec::USB_REQUEST_CLASS
            | spec::USB_REQUEST_RECIP_INTERFACE) as u8,
        request: MSC_REQUEST_GET_MAX_LUN,
        value: 0,
        index: interface,
        length: 1,
    };
    match device.control_transfer(setup, &mut buf).await {
        Ok(n) if n >= 1 => buf[0],
        _ => 0,
    }
}

fn scsi_inquiry(alloc_len: u8) -> [u8; 6] {
    let mut cdb = [0u8; 6];
    cdb[0] = SCSI_INQUIRY;
    cdb[4] = alloc_len;
    cdb
}

fn scsi_test_unit_ready() -> [u8; 6] {
    let mut cdb = [0u8; 6];
    cdb[0] = SCSI_TEST_UNIT_READY;
    cdb
}

fn scsi_request_sense(alloc_len: u8) -> [u8; 6] {
    let mut cdb = [0u8; 6];
    cdb[0] = SCSI_REQUEST_SENSE;
    cdb[4] = alloc_len;
    cdb
}

fn scsi_read_capacity10() -> [u8; 10] {
    let mut cdb = [0u8; 10];
    cdb[0] = SCSI_READ_CAPACITY_10;
    cdb
}

fn scsi_rw10(read: bool, lba: u32, blocks: u16) -> [u8; 10] {
    let mut cdb = [0u8; 10];
    cdb[0] = if read { SCSI_READ_10 } else { SCSI_WRITE_10 };
    cdb[2..6].copy_from_slice(&lba.to_be_bytes());
    cdb[7..9].copy_from_slice(&blocks.to_be_bytes());
    cdb
}

/// Brings the LUN ready and returns its `(lba_size, lba_count)`.
async fn probe_geometry(
    device: &UsbDevice,
    bulk_in: &Endpoint,
    bulk_out: &Endpoint,
    interface: u8,
) -> Option<(usize, u64)> {
    // Some bridges expect Get Max LUN before any command on the bulk pipes.
    let max_lun = get_max_lun(device, interface as u16).await;
    log!("USB mass storage: max LUN {max_lun}");

    let mut inquiry = [0u8; 36];
    let cdb = scsi_inquiry(inquiry.len() as u8);
    match bot_transfer(
        device,
        bulk_in,
        bulk_out,
        next_tag(),
        &cdb,
        Data::Buf(&mut inquiry, true),
    )
    .await
    {
        Ok(()) => {
            let vendor = core::str::from_utf8(&inquiry[8..16]).unwrap_or("").trim();
            let product = core::str::from_utf8(&inquiry[16..32]).unwrap_or("").trim();
            log!("USB mass storage: {vendor} {product}");
        }
        Err(e) => warn!("USB mass storage: INQUIRY failed: {:?}", e),
    }

    let mut ready = false;
    for _ in 0..16 {
        if bot_transfer(
            device,
            bulk_in,
            bulk_out,
            next_tag(),
            &scsi_test_unit_ready(),
            Data::None,
        )
        .await
        .is_ok()
        {
            ready = true;
            break;
        }
        let mut sense = [0u8; 18];
        let cdb = scsi_request_sense(sense.len() as u8);
        let _ = bot_transfer(
            device,
            bulk_in,
            bulk_out,
            next_tag(),
            &cdb,
            Data::Buf(&mut sense, true),
        )
        .await;
    }
    if !ready {
        warn!("USB mass storage: unit not ready");
    }

    let mut cap = [0u8; 8];
    if bot_transfer(
        device,
        bulk_in,
        bulk_out,
        next_tag(),
        &scsi_read_capacity10(),
        Data::Buf(&mut cap, true),
    )
    .await
    .is_err()
    {
        warn!("USB mass storage: READ CAPACITY(10) failed");
        return None;
    }
    let last_lba = u32::from_be_bytes([cap[0], cap[1], cap[2], cap[3]]);
    let block_size = u32::from_be_bytes([cap[4], cap[5], cap[6], cap[7]]);
    if block_size == 0 {
        warn!("USB mass storage: invalid block size");
        return None;
    }

    let lba_count = last_lba as u64 + 1;
    log!(
        "USB mass storage: {} blocks of {} bytes ({} MiB)",
        lba_count,
        block_size,
        (lba_count * block_size as u64) / (1024 * 1024),
    );
    Some((block_size as usize, lba_count))
}

struct MassStorage {
    device: Arc<UsbDevice>,
    bulk_in: Endpoint,
    bulk_out: Endpoint,
    lba_size: usize,
    lba_count: u64,
    bot: Mutex<()>,
    minor: u32,
}

impl BlockDevice for MassStorage {
    fn get_lba_size(&self) -> usize {
        self.lba_size
    }

    fn lba_count(&self) -> u64 {
        self.lba_count
    }

    fn submit_io(&self, io: &mut BlockIo) -> EResult<BlockCompletion> {
        let Some(end_lba) = io.lba().checked_add(io.num_lbas() as u64) else {
            return Err(Errno::EOVERFLOW);
        };
        if end_lba > self.lba_count {
            return match io.op() {
                BlockOp::Read => Ok(BlockCompletion { lbas: 0 }),
                BlockOp::Write => Err(Errno::ENOSPC),
            };
        }
        if io.lba() > u32::MAX as u64 {
            return Err(Errno::EIO);
        }

        let seg = io.first_segment();

        let to_boundary = 0x1_0000 - (seg.phys().value() & 0xffff);
        let max_lbas = (MAX_BOT_BYTES.min(to_boundary) / self.lba_size).max(1);
        let transfer_lbas = io.num_lbas().min(max_lbas);
        let bytes = transfer_lbas * self.lba_size;

        let is_read = io.op() == BlockOp::Read;
        let cdb = scsi_rw10(is_read, io.lba() as u32, transfer_lbas as u16);

        let _guard = self.bot.lock();
        CpuData::get().scheduler.block_on(bot_transfer(
            &self.device,
            &self.bulk_in,
            &self.bulk_out,
            next_tag(),
            &cdb,
            Data::Phys(seg.phys(), bytes, is_read),
        ))?;

        Ok(BlockCompletion {
            lbas: transfer_lbas,
        })
    }
}

impl Device for MassStorage {
    fn open(self: Arc<Self>, _flags: OpenFlags) -> EResult<Arc<dyn FileOps>> {
        Ok(self.clone())
    }

    fn major(&self) -> u32 {
        MSC_MAJOR
    }

    fn minor(&self) -> u32 {
        self.minor
    }
}
