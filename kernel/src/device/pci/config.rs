use super::{
    config,
    device::PciBar,
    generic::{CAPABILITIES_PTR, REG13},
};
use crate::{
    arch,
    irq::{IrqLine, Polarity, TriggerMode},
    memory::{
        Field, MmioView, PhysAddr, UnsafeMemoryView, VmCacheType,
        view::{BitValue, MemoryView, Register},
    },
    posix::errno::{EResult, Errno},
    util::{align_down, once::Once},
};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{fmt::Display, marker::PhantomData};
use num_traits::{FromBytes, NumCast, PrimInt, ToBytes};

pub mod common {
    use crate::memory::view::{Field, Register};

    pub const REG0: Register<u32> = Register::new(0x00).with_le();
    pub const VENDOR_ID: Field<u32, u16> = Field::new(REG0, 0);
    pub const DEVICE_ID: Field<u32, u16> = Field::new(REG0, 2);

    pub const REG1: Register<u32> = Register::new(0x04).with_le();
    pub const COMMAND: Field<u32, u16> = Field::new(REG1, 0);
    pub const STATUS: Field<u32, u16> = Field::new(REG1, 2);

    pub const REG2: Register<u32> = Register::new(0x08).with_le();
    pub const PROG_IF: Field<u32, u8> = Field::new(REG2, 0x01);
    pub const SUB_CLASS: Field<u32, u8> = Field::new(REG2, 0x02);
    pub const CLASS_CODE: Field<u32, u8> = Field::new(REG2, 0x03);

    pub const REG3: Register<u32> = Register::new(0x0C).with_le();
}

pub mod generic {
    use crate::memory::view::{Field, Register};

    pub const BAR0: Register<u32> = Register::new(0x10).with_le();
    pub const BAR1: Register<u32> = Register::new(0x14).with_le();
    pub const BAR2: Register<u32> = Register::new(0x18).with_le();
    pub const BAR3: Register<u32> = Register::new(0x1C).with_le();
    pub const BAR4: Register<u32> = Register::new(0x20).with_le();
    pub const BAR5: Register<u32> = Register::new(0x24).with_le();

    pub const CARDBUS_CIS_PTR: Register<u32> = Register::new(0x28).with_le();

    pub const REG11: Register<u32> = Register::new(0x2C).with_le();

    pub const EXPANSION_ROM: Register<u32> = Register::new(0x30).with_le();

    pub const REG13: Register<u32> = Register::new(0x34).with_le();
    pub const CAPABILITIES_PTR: Field<u32, u8> = Field::new(REG13, 0);

    pub const REG14: Register<u32> = Register::new(0x3C).with_le();
    pub const INTERRUPT_LINE: Field<u32, u8> = Field::new(REG14, 0);
    pub const INTERRUPT_PIN: Field<u32, u8> = Field::new(REG14, 1);
    pub const MIN_GRANT: Field<u32, u8> = Field::new(REG14, 2);
    pub const MAX_LATENCY: Field<u32, u8> = Field::new(REG14, 3);
}

#[derive(Copy, Clone, Debug)]
pub struct Address {
    pub segment: u16,
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
}

impl Display for Address {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "{:04x}:{:02x}:{:02x}.{:02x}",
            self.segment, self.bus, self.slot, self.function
        ))
    }
}

pub trait Access {
    fn segment(&self) -> u16;
    fn start_bus(&self) -> u8;
    fn end_bus(&self) -> u8;
    fn read32(&self, addr: Address, offset: u32) -> u32;
    fn write32(&self, addr: Address, offset: u32, value: u32);
}

impl dyn Access + '_ {
    /// Returns a [`MemoryView`] that can be used to access the device config space.
    pub fn view_for_device(&self, address: Address) -> Option<DeviceView<'_>> {
        self.decodes(address).then_some(DeviceView {
            access: self,
            address,
        })
    }

    /// Returns true if this [`Access`] contains the device as addressed by `address`.
    pub fn decodes(&self, address: Address) -> bool {
        self.segment() == address.segment
            && address.bus >= self.start_bus()
            && address.bus <= self.end_bus()
    }

    pub fn read8(&self, addr: Address, offset: u32) -> u8 {
        let aligned = align_down(offset, size_of::<u32>() as u32);
        let reg = self.read32(addr, aligned);
        (reg >> ((offset - aligned) * 8)) as u8
    }

    pub fn read16(&self, addr: Address, offset: u32) -> u16 {
        assert!(offset.is_multiple_of(2));
        let aligned = align_down(offset, size_of::<u32>() as u32);
        let reg = self.read32(addr, aligned);
        (reg >> ((offset - aligned) * 8)) as u16
    }

    pub fn write8(&self, addr: Address, offset: u32, value: u8) {
        let aligned = align_down(offset, size_of::<u32>() as u32);
        let mut reg = self.read32(addr, aligned);
        reg &= !(0xFF << ((offset - aligned) * 8));
        reg |= (value as u32) << ((offset - aligned) * 8);
        self.write32(addr, aligned, reg);
    }

    pub fn write16(&self, addr: Address, offset: u32, value: u16) {
        assert!(offset.is_multiple_of(2));
        let aligned = align_down(offset, size_of::<u32>() as u32);
        let mut reg = self.read32(addr, aligned);
        reg &= !(0xFFFF << ((offset - aligned) * 8));
        reg |= (value as u32) << ((offset - aligned) * 8);
        self.write32(addr, aligned, reg);
    }
}

#[derive(Clone)]
pub struct DeviceView<'a> {
    access: &'a dyn Access,
    address: Address,
}

impl<'a> DeviceView<'a> {
    pub fn access(&self) -> &'a dyn Access {
        self.access
    }

    pub fn address(&self) -> Address {
        self.address
    }

    pub fn capabilities(&mut self) -> CapIter<'_, 'a> {
        CapIter {
            ptr: self
                .read_reg(REG13)
                .map(|x| x.read_field(CAPABILITIES_PTR).value())
                .unwrap(),
            view: self,
        }
    }

    pub fn bar(&self, index: usize) -> Option<PciBar> {
        let bar_offset = config::generic::BAR0.offset() + index * size_of::<u32>();
        let bar = self.access.read32(self.address, bar_offset as u32);

        let is_mmio = bar & 0x1 == 0x0;
        let is_mmio64 = is_mmio && ((bar >> 1) & 0x3) == 0x2;
        let is_prefetchable = is_mmio && (bar & (1 << 3) != 0);

        let command_register = self
            .access
            .read16(self.address, config::common::COMMAND.byte_offset() as u32);

        // Disable IO and memory decoding while probing BAR sizes.
        self.access.write16(
            self.address,
            config::common::COMMAND.byte_offset() as u32,
            command_register & !(1 << 0 | 1 << 1),
        );

        // Probe the size of this BAR.
        self.access
            .write32(self.address, bar_offset as u32, 0xFFFF_FFFF);

        let new_bar = self.access.read32(self.address, bar_offset as u32);

        // Restore the original BAR value.
        self.access.write32(self.address, bar_offset as u32, bar);

        let kind = if is_mmio {
            let address = (bar & 0xFFFF_FFF0) as usize;
            let size = (!(new_bar & 0xFFFF_FFF0) + 1) as usize;

            if is_mmio64 {
                assert!(index + 1 < 6);

                let next_bar_offset = bar_offset + size_of::<u32>();
                let next_bar = self.access.read32(self.address, next_bar_offset as u32);

                PciBar::Mmio64 {
                    address: (next_bar as u64) << 32 | (address as u64),
                    size,
                    prefetchable: is_prefetchable,
                }
            } else {
                PciBar::Mmio32 {
                    address: address as u32,
                    size,
                    prefetchable: is_prefetchable,
                }
            }
        } else {
            let address = (bar & 0x0000_FFF0) as usize;
            let size = (!(new_bar & 0xFFFF_FFFC) + 1) as usize;

            PciBar::Io {
                address: address as u16,
                size,
            }
        };

        // Restore the command register.
        self.access.write16(
            self.address,
            config::common::COMMAND.byte_offset() as u32,
            command_register,
        );

        kind.is_valid().then_some(kind)
    }

    pub fn setup_msix(&mut self) -> EResult<Arc<dyn IrqLine>> {
        let (bir, table_offset) = {
            let mut found = None;
            for mut cap in self.capabilities() {
                if let Some(msix) = cap.msix() {
                    found = Some(msix);
                    break;
                }
            }
            let cap = found.ok_or(Errno::ENODEV)?;
            (cap.bir(), cap.table_offset() as usize)
        };

        log!("MSI-X table at BAR{} + {:#x}", bir, table_offset);

        // Map the first MSI-X table entry (16 bytes).
        let bar = self.bar(bir as usize).ok_or(Errno::ENODEV)?;
        let bar_addr = match bar {
            PciBar::Mmio32 { address, .. } => address as usize,
            PciBar::Mmio64 { address, .. } => address as usize,
            _ => return Err(Errno::EINVAL),
        };

        let table_view = unsafe {
            MmioView::new(
                PhysAddr::new(bar_addr + table_offset),
                16,
                VmCacheType::Uncacheable,
            )
        };

        let msi = arch::irq::allocate_msi().ok_or(Errno::ENOMEM)?;
        let line = msi.clone() as Arc<dyn IrqLine>;
        line.program(Some((TriggerMode::Edge, Polarity::High)));

        // Write entry 0: address (lo/hi), data, vector control = 0 (unmasked).
        let msg_addr = msi.msg_addr().value() as u64;
        let msg_data = msi.msg_data();
        unsafe {
            let addr_lo: Register<u32> = Register::new(0x00).with_le();
            let addr_hi: Register<u32> = Register::new(0x04).with_le();
            let data_reg: Register<u32> = Register::new(0x08).with_le();
            let ctrl_reg: Register<u32> = Register::new(0x0C).with_le();
            table_view.write_reg(addr_lo, msg_addr as u32);
            table_view.write_reg(addr_hi, (msg_addr >> 32) as u32);
            table_view.write_reg(data_reg, msg_data);
            table_view.write_reg(ctrl_reg, 0u32);
        }

        // Enable MSI-X on the device.
        for mut cap in self.capabilities() {
            if let Some(mut msix) = cap.msix() {
                msix.set_state(true);
                break;
            }
        }

        Ok(line)
    }

    pub fn setup_msi(&mut self) -> EResult<Arc<dyn IrqLine>> {
        if !self.capabilities().any(|mut cap| cap.msi().is_some()) {
            return Err(Errno::ENODEV);
        }

        log!("Using MSI (no MSI-X available)");

        let msi = arch::irq::allocate_msi().ok_or(Errno::ENOMEM)?;
        let line = msi.clone() as Arc<dyn IrqLine>;
        line.program(Some((TriggerMode::Edge, Polarity::High)));

        for mut cap in self.capabilities() {
            if let Some(mut msi_cap) = cap.msi() {
                msi_cap.program(msi.msg_addr().value() as u64, msi.msg_data() as u16);
                break;
            }
        }

        Ok(line)
    }

    pub fn setup_irq(&mut self) -> EResult<Arc<dyn IrqLine>> {
        match self.setup_msix() {
            Ok(line) => Ok(line),
            Err(Errno::ENODEV) => self.setup_msi(),
            Err(e) => Err(e),
        }
    }
}

impl<'a> MemoryView for DeviceView<'a> {
    fn read_reg<T: PrimInt + FromBytes>(&self, reg: Register<T>) -> Option<BitValue<T>>
    where
        T::Bytes: Default,
    {
        if size_of::<T>() != size_of::<u32>() {
            return None;
        }
        Some(BitValue::new(T::from(
            self.access.read32(self.address, reg.offset() as u32),
        )?))
    }

    fn write_reg<T: PrimInt + ToBytes>(&mut self, reg: Register<T>, value: T) -> Option<()>
    where
        T::Bytes: Default,
    {
        if size_of::<T>() != size_of::<u32>() {
            return None;
        }
        let value = <u32 as NumCast>::from(value)?;
        self.access
            .write32(self.address, reg.offset() as u32, value);
        Some(())
    }
}

pub struct EcamPciAccess {
    pub segment: u16,
    pub start_bus: u8,
    pub end_bus: u8,
    pub base: *mut u32,
}

impl Access for EcamPciAccess {
    fn segment(&self) -> u16 {
        self.segment
    }

    fn start_bus(&self) -> u8 {
        self.start_bus
    }

    fn end_bus(&self) -> u8 {
        self.end_bus
    }

    fn read32(&self, addr: Address, offset: u32) -> u32 {
        unsafe {
            self.base
                .byte_add(
                    (addr.bus as usize) << 20
                        | (addr.slot as usize) << 15
                        | (addr.function as usize) << 12
                        | offset as usize & 0xFFF,
                )
                .read_volatile()
        }
    }

    fn write32(&self, addr: Address, offset: u32, value: u32) {
        unsafe {
            self.base
                .byte_add(
                    (addr.bus as usize) << 20
                        | (addr.slot as usize) << 15
                        | (addr.function as usize) << 12
                        | offset as usize & 0xFFF,
                )
                .write_volatile(value)
        }
    }
}

pub struct CapIter<'a, 'b> {
    view: &'a DeviceView<'b>,
    ptr: u8,
}

impl<'a, 'b> Iterator for CapIter<'a, 'b> {
    type Item = Capability<'a, ()>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.ptr == 0 {
            return None;
        }

        let cur = self.ptr;
        let reg: Register<u32> = Register::new(self.ptr as usize);
        let field: Field<_, u8> = Field::new_bits(reg, 8..=15);

        self.ptr = self
            .view
            .read_reg(reg)
            .map(|x| x.read_field(field).value())
            .unwrap();

        Some(Capability {
            view: self.view.clone(),
            cap: cur,
            _p: PhantomData,
        })
    }
}

pub struct Capability<'a, T> {
    view: DeviceView<'a>,
    cap: u8,
    _p: PhantomData<T>,
}

impl<'a> Capability<'a, ()> {
    pub fn id(&self) -> u8 {
        let reg: Register<u32> = Register::new(self.cap as usize);
        let field: Field<_, u8> = Field::new_bits(reg, 0..=7);
        self.view
            .read_reg(reg)
            .map(|x| x.read_field(field).value())
            .unwrap()
    }

    pub fn msi(&mut self) -> Option<Capability<'a, MsiCapability>> {
        (self.id() == 0x05).then_some(Capability {
            view: self.view.clone(),
            cap: self.cap,
            _p: PhantomData,
        })
    }

    pub fn msix(&mut self) -> Option<Capability<'a, MsiXCapability>> {
        (self.id() == 0x11).then_some(Capability {
            view: self.view.clone(),
            cap: self.cap,
            _p: PhantomData,
        })
    }
}

/// Capability for MSIs
pub struct MsiCapability;
impl<'a> Capability<'a, MsiCapability> {
    /// Whether the capability uses a 64-bit message address.
    fn is_64bit(&self) -> bool {
        let reg: Register<u32> = Register::new(self.cap as usize);
        let field: Field<_, u8> = Field::new_bits(reg, 23..=23);
        self.view
            .read_reg(reg)
            .map(|x| x.read_field(field).value())
            .unwrap_or(0)
            != 0
    }

    pub fn program(&mut self, addr: u64, data: u16) {
        let base = self.cap as usize;
        let addr_lo: Register<u32> = Register::new(base + 0x04);
        self.view.write_reg(addr_lo, addr as u32);

        let data_off = if self.is_64bit() {
            let addr_hi: Register<u32> = Register::new(base + 0x08);
            self.view.write_reg(addr_hi, (addr >> 32) as u32);
            0x0C
        } else {
            0x08
        };
        let data_reg: Register<u32> = Register::new(base + data_off);
        self.view.write_reg(data_reg, data as u32);

        let ctrl: Register<u32> = Register::new(base);
        let enable: Field<_, u8> = Field::new_bits(ctrl, 16..=16);
        let multiple_message_enable: Field<_, u8> = Field::new_bits(ctrl, 20..=22);
        let value = self
            .view
            .read_reg(ctrl)
            .unwrap()
            .write_field(multiple_message_enable, 0u8)
            .write_field(enable, 1u8);
        self.view.write_reg(ctrl, value.value());
    }
}

/// Capability for MSI-Xs
pub struct MsiXCapability;
impl<'a> Capability<'a, MsiXCapability> {
    pub fn set_state(&mut self, status: bool) {
        let reg: Register<u32> = Register::new(self.cap as usize);
        let enable: Field<_, u8> = Field::new_bits(reg, 31..=31);
        let function_mask: Field<_, u8> = Field::new_bits(reg, 30..=30);
        let old = self
            .view
            .read_reg(reg)
            .unwrap()
            .write_field(enable, status as u8)
            .write_field(function_mask, false as u8);
        self.view.write_reg(reg, old.value());
    }

    pub fn bir(&self) -> u8 {
        let reg: Register<u32> = Register::new(self.cap as usize + 4);
        let field: Field<_, u8> = Field::new_bits(reg, 0..=2);
        self.view
            .read_reg(reg)
            .map(|x| x.read_field(field).value())
            .unwrap()
    }

    pub fn table_offset(&self) -> u32 {
        let reg: Register<u32> = Register::new(self.cap as usize + 4);
        self.view.read_reg(reg).map(|x| x.value() & !0x7).unwrap()
    }
}

// TODO: Use a SpinMutex instead.
pub static ACCESS: Once<Vec<Box<dyn Access>>> = Once::new();
