#![no_std]

use core::ptr::{read_volatile, write_volatile};
use zinnia::{
    alloc::{boxed::Box, sync::Arc},
    arch, clock,
    device::{
        net::{dev::register_nic, l2::mac::MacAddr, nic::NicDevice},
        pci::{DeviceView, Driver, PciBar, PciVariant},
    },
    error,
    irq::{IrqHandler, Status},
    log,
    memory::{AllocFlags, BitValue, MmioView, OwnedPhysPages, PhysAddr, VmCacheType},
    posix::errno::{EResult, Errno},
    util::{event::Event, mutex::spin::SpinMutex},
    warn,
};

mod controller;
mod regs;

use controller::IgcController;
use regs::*;

const RING_SIZE: usize = 256;
const BUF_SIZE: usize = 2048;
const MAX_FRAME_LEN: usize = 1518;

struct Ring {
    descs: OwnedPhysPages,
    bufs: OwnedPhysPages,
}

impl Ring {
    fn new() -> EResult<Self> {
        let page_size = arch::virt::get_page_size();
        Ok(Self {
            descs: OwnedPhysPages::new(
                (RING_SIZE * DESC_SIZE).div_ceil(page_size),
                AllocFlags::empty(),
            )?,
            bufs: OwnedPhysPages::new(
                (RING_SIZE * BUF_SIZE).div_ceil(page_size),
                AllocFlags::empty(),
            )?,
        })
    }

    fn desc(&self, i: usize) -> *mut u8 {
        debug_assert!(i < RING_SIZE);
        unsafe { self.descs.as_hhdm::<u8>().add(i * DESC_SIZE) }
    }

    fn buf_phys(&self, i: usize) -> u64 {
        self.bufs.phys().value() as u64 + (i * BUF_SIZE) as u64
    }

    fn buf(&self, i: usize) -> *mut u8 {
        unsafe { self.bufs.as_hhdm::<u8>().add(i * BUF_SIZE) }
    }
}

struct RxState {
    ring: Ring,
    /// Next descriptor to check for a received frame.
    ntc: usize,
}

impl RxState {
    fn arm(&self, i: usize) {
        let d = self.ring.desc(i);
        unsafe {
            write_volatile(d as *mut u64, self.ring.buf_phys(i));
            write_volatile(d.add(RXD_STATUS_OFFSET) as *mut u64, 0);
        }
    }
}

struct TxState {
    ring: Ring,
    /// Next slot to place a frame in.
    ntu: usize,
    /// Oldest slot not yet reaped.
    ntc: usize,
    outstanding: usize,
}

impl TxState {
    /// Releases ring slots whose descriptors the hardware has written back.
    fn reap(&mut self) {
        while self.outstanding > 0 {
            let d = self.ring.desc(self.ntc);
            let status = unsafe { read_volatile(d.add(TXD_STATUS_OFFSET) as *const u32) };
            if status & TXD_STAT_DD == 0 {
                break;
            }
            self.ntc = (self.ntc + 1) % RING_SIZE;
            self.outstanding -= 1;
        }
    }
}

struct Controller {
    hw: IgcController,
    mac: MacAddr,
    rx: SpinMutex<RxState>,
    tx: SpinMutex<TxState>,
    rx_event: Event,
}

impl NicDevice for Controller {
    fn mac(&self) -> MacAddr {
        self.mac
    }

    fn recv(&self, frame: &mut [u8]) -> EResult<usize> {
        loop {
            let guard = self.rx_event.guard();
            {
                let mut rx = self.rx.lock();
                let i = rx.ntc;
                let d = rx.ring.desc(i);
                let len = unsafe { read_volatile(d.add(RXD_LENGTH_OFFSET) as *const u16) } as usize;
                if len != 0 {
                    let status = unsafe { read_volatile(d.add(RXD_STATUS_OFFSET) as *const u32) };
                    let n = len.min(frame.len()).min(BUF_SIZE);
                    unsafe {
                        core::ptr::copy_nonoverlapping(rx.ring.buf(i), frame.as_mut_ptr(), n);
                    }

                    rx.arm(i);
                    self.hw.write(RDT0, i as u32);
                    rx.ntc = (i + 1) % RING_SIZE;

                    if status & RXD_STAT_EOP == 0 {
                        warn!("Dropping RX frame without EOP");
                        continue;
                    }
                    return Ok(n);
                }
            }
            guard.wait();
        }
    }

    fn send(&self, frame: &[u8]) -> EResult<()> {
        if frame.is_empty() || frame.len() > MAX_FRAME_LEN {
            return Err(Errno::EMSGSIZE);
        }

        let mut tx = self.tx.lock();
        tx.reap();
        if tx.outstanding == RING_SIZE - 1 {
            // Ring full.
            let deadline =
                clock::get_elapsed().saturating_add(core::time::Duration::from_millis(10));
            loop {
                tx.reap();
                if tx.outstanding < RING_SIZE - 1 {
                    break;
                }
                if clock::get_elapsed() >= deadline {
                    return Err(Errno::ENOBUFS);
                }
                core::hint::spin_loop();
            }
        }

        let i = tx.ntu;
        let d = tx.ring.desc(i);
        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), tx.ring.buf(i), frame.len());
            write_volatile(d as *mut u64, tx.ring.buf_phys(i));
            write_volatile(
                d.add(TXD_CMD_OFFSET) as *mut u32,
                frame.len() as u32
                    | ADVTXD_DTYP_DATA
                    | ADVTXD_DCMD_DEXT
                    | ADVTXD_DCMD_IFCS
                    | ADVTXD_DCMD_EOP
                    | ADVTXD_DCMD_RS,
            );
            write_volatile(
                d.add(TXD_OLINFO_OFFSET) as *mut u32,
                (frame.len() as u32) << ADVTXD_PAYLEN_SHIFT,
            );
        }
        tx.ntu = (i + 1) % RING_SIZE;
        tx.outstanding += 1;
        self.hw.write(TDT0, tx.ntu as u32);
        Ok(())
    }
}

struct IgcIrqHandler {
    controller: Arc<Controller>,
}

impl IrqHandler for IgcIrqHandler {
    fn raise(&mut self) -> Status {
        let hw = &self.controller.hw;
        let icr = hw.read(ICR);
        if icr.read_field(icr::LSC).value() != 0 {
            let (up, speed, full_duplex) = hw.link_status();
            if up {
                log!(
                    "Link up, {} Mb/s, {} duplex",
                    speed,
                    if full_duplex { "full" } else { "half" }
                );
            } else {
                log!("Link down");
            }
        }

        hw.write(EIMS, 1);

        self.controller.rx_event.wake_all();
        Status::Handled
    }
}

fn probe(variant: &PciVariant, mut view: DeviceView<'static>) -> EResult<()> {
    let address = view.address();
    log!(
        "Probing compatible NIC \"{}\" on {}",
        DEV_NAMES[variant.data.unwrap()],
        address
    );

    // Enable memory decode and bus mastering, disable legacy INTx.
    let cmd = view.access().read16(view.address(), 0x04);
    view.access()
        .write16(address, 0x04, cmd | (1 << 1) | (1 << 2) | (1 << 10));

    // Disable ASPM L1.2
    let access = view.access();
    let mut off: u32 = 0x100;
    for _ in 0..64 {
        let hdr = access.read32(address, off);
        if hdr == 0 || hdr == !0 {
            break;
        }
        if hdr & 0xFFFF == 0x001E {
            let ctl1 = access.read32(address, off + 8);
            if ctl1 & 0x5 != 0 {
                access.write32(address, off + 8, ctl1 & !0x5);
            }
            break;
        }
        off = hdr >> 20;
        if off < 0x100 || off & 3 != 0 {
            break;
        }
    }

    let bar = view.bar(0).ok_or(Errno::ENXIO)?;
    let (bar_addr, bar_size) = match bar {
        PciBar::Mmio32 { address, size, .. } => (address as usize, size),
        PciBar::Mmio64 { address, size, .. } => (address as usize, size),
        PciBar::Io { .. } => return Err(Errno::EINVAL),
    };

    if bar_size < 0x1_0000 {
        error!("{address}: BAR0 too small ({:#x} bytes)", bar_size);
        return Err(Errno::ENODEV);
    }

    let hw = IgcController::new(unsafe {
        MmioView::new(PhysAddr::new(bar_addr), bar_size, VmCacheType::Uncacheable)
    });

    hw.reset();

    let irq_line = view.setup_msix()?;

    let mac_bytes = hw.read_mac();
    if mac_bytes == [0x00; 6] || mac_bytes == [0xFF; 6] {
        error!("{address}: No valid MAC address in NVM");
        return Err(Errno::ENODEV);
    }

    let mac = MacAddr::new(&mac_bytes);
    log!("{address}: MAC address {}", mac);

    hw.init_rx_addrs(&mac_bytes);

    if let Err(e) = hw.phy_power_up_autoneg() {
        warn!("{address}: PHY restart failed: {:?}", e);
    }
    hw.setup_link();

    // RX ring.
    let rx = RxState {
        ring: Ring::new()?,
        ntc: 0,
    };
    for i in 0..RING_SIZE {
        rx.arm(i);
    }
    let rx_phys = rx.ring.descs.phys().value() as u64;
    hw.write(RXDCTL0, 0);
    hw.write(RDBAL0, rx_phys as u32);
    hw.write(RDBAH0, (rx_phys >> 32) as u32);
    hw.write(RDLEN0, (RING_SIZE * DESC_SIZE) as u32);
    hw.write(RDH0, 0);
    hw.write(RDT0, 0);
    hw.write(
        SRRCTL0,
        BitValue::new(0u32)
            .write_field(srrctl0::BSIZEPKT, (BUF_SIZE >> 10) as u8)
            .write_field(srrctl0::BSIZEHDR, (256 >> 6) as u8)
            .write_field(srrctl0::DESCTYPE, srrctl0::DESCTYPE_ADV_ONEBUF)
            .value(),
    );
    hw.write(
        RXDCTL0,
        BitValue::new(0u32)
            .write_field(rxdctl0::PTHRESH, 8)
            .write_field(rxdctl0::HTHRESH, 8)
            .write_field(rxdctl0::WTHRESH, 4)
            .write_field(rxdctl0::QUEUE_ENABLE, 1)
            .value(),
    );

    if !hw.poll(10_000, || {
        hw.read(RXDCTL0).read_field(rxdctl0::QUEUE_ENABLE).value() != 0
    }) {
        warn!("{address}: RX queue 0 did not enable");
    }

    hw.write(
        RCTL,
        BitValue::new(0u32)
            .write_field(rctl::EN, 1)
            .write_field(rctl::BAM, 1)
            .write_field(rctl::SECRC, 1)
            .value(),
    );
    hw.wrfl();
    hw.write(RDT0, (RING_SIZE - 1) as u32);

    // TX ring.
    let tx = TxState {
        ring: Ring::new()?,
        ntu: 0,
        ntc: 0,
        outstanding: 0,
    };

    let tx_phys = tx.ring.descs.phys().value() as u64;
    hw.write(TXDCTL0, 0);
    hw.wrfl();
    hw.write(TDBAL0, tx_phys as u32);
    hw.write(TDBAH0, (tx_phys >> 32) as u32);
    hw.write(TDLEN0, (RING_SIZE * DESC_SIZE) as u32);
    hw.write(TDH0, 0);
    hw.write(TDT0, 0);
    hw.write(
        TCTL,
        BitValue::new(0u32)
            .write_field(tctl::EN, 1)
            .write_field(tctl::PSP, 1)
            .write_field(tctl::CT, COLLISION_THRESHOLD)
            .write_field(tctl::RTLC, 1)
            .value(),
    );
    hw.write(
        TXDCTL0,
        BitValue::new(0u32)
            .write_field(txdctl0::PTHRESH, 8)
            .write_field(txdctl0::HTHRESH, 1)
            .write_field(txdctl0::WTHRESH, 0)
            .write_field(txdctl0::QUEUE_ENABLE, 1)
            .value(),
    );
    if !hw.poll(10_000, || {
        hw.read(TXDCTL0).read_field(txdctl0::QUEUE_ENABLE).value() != 0
    }) {
        warn!("{address}: TX queue 0 did not enable");
    }

    let controller = Arc::new(Controller {
        hw,
        mac,
        rx: SpinMutex::new(rx),
        tx: SpinMutex::new(tx),
        rx_event: Event::new(),
    });
    let hw = &controller.hw;

    // Single MSI-X vector for everything.
    hw.write(
        GPIE,
        BitValue::new(0u32)
            .write_field(gpie::NSICR, 1)
            .write_field(gpie::MSIX_MODE, 1)
            .write_field(gpie::EIAME, 1)
            .write_field(gpie::PBA, 1)
            .value(),
    );
    hw.write(
        IVAR0,
        BitValue::new(0u32)
            .write_field(ivar0::RX_Q0, IVAR_VALID)
            .write_field(ivar0::TX_Q0, IVAR_VALID)
            .value(),
    );
    hw.write(
        IVAR_MISC,
        BitValue::new(0u32)
            .write_field(ivar_misc::OTHER, IVAR_VALID)
            .value(),
    );
    hw.write(EITR0, START_ITR);

    irq_line.attach(Box::new(IgcIrqHandler {
        controller: controller.clone(),
    }));
    irq_line.unmask();

    hw.read(ICR);
    hw.write(EIAC, 1);
    hw.write(EIAM, 1);
    hw.write(EIMS, 1);
    hw.write(IMS, BitValue::new(0u32).write_field(ims::LSC, 1).value());
    hw.wrfl();

    register_nic(controller)?;
    Ok(())
}

const V: PciVariant = PciVariant::new().vendor(0x8086);

const DEV_NAMES: &[&str] = &[
    "I226-V", "I226-LM", "I226-IT", "I225-LM", "I225-V", "I225-IT",
];

static DRIVER: Driver = Driver {
    name: "igc",
    variants: &[
        V.device(0x125C).with_data(0), // I226-V
        V.device(0x125B).with_data(1), // I226-LM
        V.device(0x125D).with_data(2), // I226-IT
        V.device(0x15F2).with_data(3), // I225-LM
        V.device(0x15F3).with_data(4), // I225-V
        V.device(0x0D9F).with_data(5), // I225-IT
    ],
    probe,
};

zinnia::module!("Intel I225/I226 NIC driver", "Marvin Friedrich", main);

fn main(_cmdline: &str) {
    match DRIVER.register() {
        Ok(_) => (),
        Err(e) => error!("Unable to load igc driver: {:?}", e),
    }
}
