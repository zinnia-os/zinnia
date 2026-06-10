use crate::regs::*;
use zinnia::{
    clock, log,
    memory::{BitValue, MmioView, Register, UnsafeMemoryView},
    posix::errno::{EResult, Errno},
    warn,
};

pub struct IgcController {
    regs: MmioView,
}

impl IgcController {
    pub fn new(regs: MmioView) -> Self {
        Self { regs }
    }

    pub fn read(&self, reg: Register<u32>) -> BitValue<u32> {
        unsafe { self.regs.read_reg(reg) }.expect("MMIO read out of bounds")
    }

    pub fn write(&self, reg: Register<u32>, value: u32) {
        unsafe { self.regs.write_reg(reg, value) }.expect("MMIO write out of bounds");
    }

    pub fn update(&self, reg: Register<u32>, f: impl FnOnce(BitValue<u32>) -> BitValue<u32>) {
        self.write(reg, f(self.read(reg)).value());
    }

    pub fn wrfl(&self) {
        self.read(STATUS);
    }

    pub fn poll(&self, timeout_us: usize, mut cond: impl FnMut() -> bool) -> bool {
        let deadline = clock::get_elapsed().saturating_add(timeout_us * 1_000);
        loop {
            if cond() {
                return true;
            }
            if clock::get_elapsed() >= deadline {
                return false;
            }
            core::hint::spin_loop();
        }
    }

    pub fn reset(&self) {
        self.write(IMC, !0);
        self.read(ICR);

        self.write(RCTL, 0);
        self.write(TCTL, BitValue::new(0u32).write_field(tctl::PSP, 1).value());
        self.wrfl();
        _ = clock::block_ns(10_000_000);

        self.update(CTRL, |v| v.write_field(ctrl::GIO_MASTER_DISABLE, 1));
        if !self.poll(800_000, || {
            self.read(STATUS)
                .read_field(status::GIO_MASTER_ENABLE)
                .value()
                == 0
        }) {
            warn!("PCIe master disable timed out, resetting anyway");
        }

        self.update(CTRL, |v| v.write_field(ctrl::RST, 1));
        _ = clock::block_ns(1_000_000);

        if !self.poll(20_000, || {
            self.read(EECD).read_field(eecd::AUTO_RD).value() != 0
        }) {
            warn!("NVM auto-read did not complete after reset");
        }

        self.write(IMC, !0);
        self.read(ICR);
    }

    pub fn read_mac(&self) -> [u8; 6] {
        let ral = self.read(RAL0).value();
        let rah = self.read(RAH0).value();
        [
            ral as u8,
            (ral >> 8) as u8,
            (ral >> 16) as u8,
            (ral >> 24) as u8,
            rah as u8,
            (rah >> 8) as u8,
        ]
    }

    pub fn init_rx_addrs(&self, mac: &[u8; 6]) {
        let ral_val = u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]);
        let rah_val =
            BitValue::new((mac[4] as u32) | ((mac[5] as u32) << 8)).write_field(rah0::AV, 1);

        self.write(RAL0, ral_val);
        self.wrfl();
        self.write(RAH0, rah_val.value());
        self.wrfl();

        for n in 1..RAR_COUNT {
            self.write(ral(n), 0);
            self.wrfl();
            self.write(rah(n), 0);
            self.wrfl();
        }

        for i in 0..MTA_COUNT {
            self.write(mta(i), 0);
        }
        self.wrfl();
    }

    fn get_hw_semaphore(&self) -> EResult<()> {
        let smbi_free = || self.read(SWSM).read_field(swsm::SMBI).value() == 0;
        if !self.poll(100_000, smbi_free) {
            warn!("SMBI stuck, force-releasing hardware semaphore");
            self.put_hw_semaphore();
            if !self.poll(100_000, smbi_free) {
                return Err(Errno::ETIMEDOUT);
            }
        }

        let ok = self.poll(100_000, || {
            self.update(SWSM, |v| v.write_field(swsm::SWESMBI, 1));
            self.read(SWSM).read_field(swsm::SWESMBI).value() != 0
        });
        if !ok {
            self.put_hw_semaphore();
            return Err(Errno::ETIMEDOUT);
        }
        Ok(())
    }

    fn put_hw_semaphore(&self) {
        self.update(SWSM, |v| {
            v.write_field(swsm::SMBI, 0).write_field(swsm::SWESMBI, 0)
        });
    }

    pub fn acquire_swfw_sync(&self, mask: u32) -> EResult<()> {
        let fwmask = mask << 16;
        for _ in 0..200 {
            self.get_hw_semaphore()?;

            let swfw_sync = self.read(SW_FW_SYNC).value();
            if swfw_sync & (mask | fwmask) == 0 {
                self.write(SW_FW_SYNC, swfw_sync | mask);
                self.put_hw_semaphore();
                return Ok(());
            }

            self.put_hw_semaphore();
            _ = clock::block_ns(5_000_000);
        }
        Err(Errno::ETIMEDOUT)
    }

    pub fn release_swfw_sync(&self, mask: u32) {
        if self.get_hw_semaphore().is_err() {
            warn!("Failed to take semaphore for SW_FW_SYNC release");
            return;
        }
        let swfw_sync = self.read(SW_FW_SYNC).value();
        self.write(SW_FW_SYNC, swfw_sync & !mask);
        self.put_hw_semaphore();
    }

    pub fn mdic_read(&self, reg: u8) -> EResult<u16> {
        let cmd = BitValue::new(0u32)
            .write_field(mdic::REGADD, reg)
            .write_field(mdic::OP, mdic::OP_READ);
        self.write(MDIC, cmd.value());
        self.mdic_wait()
    }

    pub fn mdic_write(&self, reg: u8, value: u16) -> EResult<()> {
        let cmd = BitValue::new(0u32)
            .write_field(mdic::DATA, value)
            .write_field(mdic::REGADD, reg)
            .write_field(mdic::OP, mdic::OP_WRITE);
        self.write(MDIC, cmd.value());
        self.mdic_wait().map(|_| ())
    }

    fn mdic_wait(&self) -> EResult<u16> {
        let mut last = BitValue::new(0u32);
        if !self.poll(100_000, || {
            _ = clock::block_ns(50_000);
            last = self.read(MDIC);
            last.read_field(mdic::READY).value() != 0
        }) {
            return Err(Errno::ETIMEDOUT);
        }
        if last.read_field(mdic::ERROR).value() != 0 {
            return Err(Errno::EIO);
        }
        Ok(last.read_field(mdic::DATA).value())
    }

    pub fn phy_power_up_autoneg(&self) -> EResult<()> {
        self.acquire_swfw_sync(SWFW_PHY0_SM)?;

        let result = (|| {
            let bmcr = self.mdic_read(0)?;
            if bmcr & MII_CR_POWER_DOWN != 0 {
                log!("PHY was powered down, powering up");
            }
            self.mdic_write(
                0,
                (bmcr & !MII_CR_POWER_DOWN) | MII_CR_AUTO_NEG_EN | MII_CR_RESTART_AUTO_NEG,
            )
        })();

        self.release_swfw_sync(SWFW_PHY0_SM);
        result
    }

    pub fn setup_link(&self) {
        self.update(CTRL, |v| {
            v.write_field(ctrl::SLU, 1)
                .write_field(ctrl::FRCSPD, 0)
                .write_field(ctrl::FRCDPX, 0)
        });
    }

    pub fn link_status(&self) -> (bool, u32, bool) {
        let s = self.read(STATUS);
        let speed = match s.read_field(status::SPEED).value() {
            0 => 10,
            1 => 100,
            _ => {
                if s.read_field(status::SPEED_2500).value() != 0 {
                    2500
                } else {
                    1000
                }
            }
        };
        (
            s.read_field(status::LU).value() != 0,
            speed,
            s.read_field(status::FD).value() != 0,
        )
    }
}
