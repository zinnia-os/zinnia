use crate::{
    arch::x86_64::{
        asm,
        consts::{self, IDT_IPI_RESCHED},
        irq::IRQ_LINES,
    },
    clock,
    irq::{self, IrqLine, IrqLineState, IrqMode, MsiLine, Polarity},
    memory::{
        BitValue, PhysAddr, UnsafeMemoryView,
        view::{MmioView, Register},
    },
    percpu::CpuData,
    system,
    util::mutex::spin::SpinMutex,
};
use alloc::{boxed::Box, sync::Arc};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub struct LocalApic {
    /// How many ticks pass in 10 milliseconds.
    ticks_per_10ms: AtomicU32,
    /// If [`Some`], points to the xAPIC MMIO space.
    /// Otherwise, it's an x2APIC.
    xapic_regs: SpinMutex<Option<MmioView>>,
}

per_cpu! {
    pub static LAPIC: LocalApic = LocalApic { ticks_per_10ms: AtomicU32::new(0), xapic_regs: SpinMutex::new(None) };
}

#[allow(unused)]
mod lapic_regs {
    use crate::memory::view::Register;

    pub const ID: Register<u32> = Register::new(0x20);
    pub const TPR: Register<u32> = Register::new(0x80);
    pub const EOI: Register<u32> = Register::new(0xB0);
    pub const LDR: Register<u32> = Register::new(0xD0);
    pub const DFR: Register<u32> = Register::new(0xE0);
    pub const SIVR: Register<u32> = Register::new(0xF0);
    pub const ESR: Register<u32> = Register::new(0x280);
    pub const ICR: Register<u32> = Register::new(0x300);
    pub const ICR_HI: Register<u32> = Register::new(0x310);
    pub const LVT_TR: Register<u32> = Register::new(0x320);
    pub const ICR_TIMER: Register<u32> = Register::new(0x380);
    pub const CCR: Register<u32> = Register::new(0x390);
    pub const DCR: Register<u32> = Register::new(0x3E0);
}

#[repr(u8)]
#[allow(unused)]
pub enum DeliveryMode {
    Fixed = 0b000,
    LowestPrio = 0b001,
    SMI = 0b010,
    NMI = 0b100,
    INIT = 0b101,
    StartUp = 0b110,
}

#[repr(u8)]
pub enum DestinationMode {
    Physical = 0,
    Logical = 1,
}

#[repr(u8)]
pub enum DeliveryStatus {
    Idle = 0,
    Pending = 1,
}

#[repr(u8)]
#[allow(unused)]
pub enum Level {
    Deassert = 0,
    Assert = 1,
}

#[repr(u8)]
#[allow(unused)]
pub enum TriggerMode {
    Edge = 0,
    Level = 1,
}

#[allow(unused)]
pub enum IpiTarget {
    /// Send an interrupt to the calling CPU.
    ThisCpu,
    /// Send an interrupt to all CPUs.
    All,
    /// Send an interrupt to all CPUs except the calling CPU.
    AllButThisCpu,
    /// Send an interrupt to a specific CPU. The value is the ID of the target [`LocalApic`].
    Specific(u32),
}

impl LocalApic {
    pub fn init() {
        let lapic = LAPIC.get();

        // Enable the APIC flag.
        let mut apic_msr = unsafe { asm::rdmsr(0x1B) };
        apic_msr |= 1 << 11;

        // Enable the x2APIC if we have it.
        *lapic.xapic_regs.lock() = {
            let cpuid = asm::cpuid(1, 0);
            if cpuid.ecx & consts::CPUID_1C_X2APIC != 0 {
                apic_msr |= 1 << 10;
                None
            } else {
                Some(unsafe { MmioView::new(PhysAddr::from(apic_msr & 0xFFFFF000), 0x1000) })
            }
        };

        unsafe { asm::wrmsr(0x1B, apic_msr) };

        // Reset the TPR.
        lapic.write_reg(lapic_regs::TPR, 0);
        // Enable APIC bit in the SIVR.
        lapic.write_reg(lapic_regs::SIVR, lapic.read_reg(lapic_regs::SIVR) | 0x100);

        if lapic.xapic_regs.lock().is_some() {
            lapic.write_reg(lapic_regs::DFR, 0xF000_0000);
            // Logical destination = LAPIC ID.
            lapic.write_reg(lapic_regs::LDR, lapic.read_reg(lapic_regs::ID));
        }

        // TODO: Parse MADT and setup NMIs.

        // Tell the APIC timer to divide by 16.
        lapic.write_reg(lapic_regs::DCR, 3);
        // Set the timer counter to the highest possible value.
        lapic.write_reg(lapic_regs::ICR_TIMER, u32::MAX as u64);

        // Sleep for 10 milliseconds.
        clock::block_ns(10_000_000)
            .expect("Unable to setup LAPIC, the kernel should have a working timer!");

        // Read how many ticks have passed in 10 ms.
        lapic.ticks_per_10ms.store(
            u32::MAX - lapic.read_reg(lapic_regs::CCR) as u32,
            Ordering::Relaxed,
        );

        // Finally, run the periodic timer interrupt.
        lapic.write_reg(lapic_regs::LVT_TR, IDT_IPI_RESCHED as u64 | 0x20000);
        lapic.write_reg(lapic_regs::DCR, 3);
        lapic.write_reg(
            lapic_regs::ICR_TIMER,
            lapic.ticks_per_10ms.load(Ordering::Relaxed) as u64,
        );
    }

    fn read_reg(&self, reg: Register<u32>) -> u64 {
        match &*self.xapic_regs.lock() {
            Some(x) => unsafe {
                if reg == lapic_regs::ICR {
                    let lo = x.read_reg(lapic_regs::ICR).unwrap().value();
                    let hi = x.read_reg(lapic_regs::ICR_HI).unwrap().value();

                    (hi as u64) << 32 | (lo as u64)
                } else {
                    x.read_reg(reg).unwrap().value().into()
                }
            },
            None => unsafe { asm::rdmsr(0x800 + (reg.offset() as u32 >> 4)) },
        }
    }

    fn write_reg(&self, reg: Register<u32>, value: u64) {
        match &mut *self.xapic_regs.lock() {
            Some(x) => unsafe {
                if reg == lapic_regs::ICR {
                    x.write_reg(lapic_regs::ICR_HI, (value >> 32) as u32)
                        .unwrap();
                    x.write_reg(lapic_regs::ICR, value as u32).unwrap();
                } else {
                    x.write_reg(reg, value as u32).unwrap()
                }
            },
            None => unsafe { asm::wrmsr(0x800 + (reg.offset() as u32 >> 4), value) },
        }
    }

    pub fn id(&self) -> u32 {
        return self.read_reg(lapic_regs::ID) as u32;
    }

    /// Signals an end of interrupt to the LAPIC.
    pub fn eoi(&self) {
        self.write_reg(lapic_regs::EOI, 0);
    }

    /// Sends an inter-processor interrupt.
    pub fn send_ipi(
        &self,
        target: IpiTarget,
        vector: u8,
        delivery_mode: DeliveryMode,
        destination_mode: DestinationMode,
        delivery_status: DeliveryStatus,
        level: Level,
        trigger_mode: TriggerMode,
    ) {
        let mut icr = vector as u64;
        icr |= (delivery_mode as u64) << 8;
        icr |= (destination_mode as u64) << 11;
        icr |= (delivery_status as u64) << 12;
        icr |= (level as u64) << 14;
        icr |= (trigger_mode as u64) << 15;
        icr |= (match target {
            IpiTarget::ThisCpu => 0b01,
            IpiTarget::All => 0b10,
            IpiTarget::AllButThisCpu => 0b11,
            IpiTarget::Specific(x) => {
                if self.xapic_regs.lock().is_some() {
                    icr |= (x as u8 as u64) << 56;
                } else {
                    icr |= (x as u64) << 32;
                }
                0b00
            }
        } as u64)
            << 18;

        self.write_reg(lapic_regs::ICR, icr);
    }

    pub fn allocate_msi(&self) -> Option<Arc<ApicMsiLine>> {
        let mut lines = IRQ_LINES.get().lock();
        let line = lines.iter_mut().position(|x| x.is_none())?;

        let msi = Arc::new(ApicMsiLine {
            state: IrqLineState::new(),
            vector: 0,
            lapic: self.id() as u8,
        });
        lines[line] = Some(msi.clone());

        Some(msi)
    }
}

pub struct ApicMsiLine {
    state: IrqLineState,
    vector: u8,
    lapic: u8,
}

impl MsiLine for ApicMsiLine {
    fn msg_addr(&self) -> PhysAddr {
        PhysAddr::new(0xFEE0_0000 | ((self.lapic as usize) << 12))
    }

    fn msg_data(&self) -> u32 {
        self.vector as u32
    }
}

impl IrqLine for ApicMsiLine {
    fn state(&self) -> &IrqLineState {
        &self.state
    }

    fn set_config(&self, trigger: irq::TriggerMode, _polarity: Polarity) -> IrqMode {
        assert!(trigger == irq::TriggerMode::Edge);
        IrqMode::EndOfInterrupt
    }

    fn mask(&self) {}

    fn unmask(&self) {}
}

#[allow(unused)]
mod ioapic_regs {
    use crate::memory::Field;
    use crate::memory::Register;

    pub const ID: u32 = 0;
    pub const VERSION: u32 = 1;
    pub const INTS: u32 = 0x10;

    pub const INDEX: Register<u32> = Register::new(0);
    pub const DATA: Register<u32> = Register::new(0x10);

    pub const VECTOR: Field<u32, u8> = Field::new_bits(DATA, 0..=7);
    pub const DELIVERY_MODE: Field<u32, u8> = Field::new_bits(DATA, 8..=10);
    pub const DELIVERY_STATUS: Field<u32, u8> = Field::new_bits(DATA, 12..=12);
    pub const ACTIVE_LOW: Field<u32, u8> = Field::new_bits(DATA, 13..=13);
    pub const REMOTE_IRR: Field<u32, u8> = Field::new_bits(DATA, 14..=14);
    pub const LEVEL_TRIGGERED: Field<u32, u8> = Field::new_bits(DATA, 15..=15);
    pub const MASKED: Field<u32, u8> = Field::new_bits(DATA, 16..=16);
    pub const DESTINATION: Field<u32, u8> = Field::new_bits(DATA, 24..=31);
}

pub struct IoApic {
    id: u8,
    gsi_base: u32,
    regs: MmioView,
}

impl IoApic {
    pub fn setup(id: u8, gsi_base: u32, addr: PhysAddr) {
        let ioapic = Arc::new(Self {
            id,
            gsi_base,
            regs: unsafe { MmioView::new(addr, 0x1000) },
        });

        let num_lines = ((ioapic.read_reg(ioapic_regs::VERSION) >> 16) & 0xFF) + 1;
        log!(
            "IOAPIC {} with {} lines, GSI base at {}",
            id,
            num_lines,
            gsi_base
        );

        for i in 0..num_lines {
            let mut slot = None;
            // Find a CPU with free IRQ lines.
            for cpu in CpuData::iter() {
                let lines = IRQ_LINES.get_for(cpu);
                for (i, line) in lines.lock().iter().enumerate() {
                    if line.is_none() {
                        slot = Some((cpu, i))
                    }
                }
            }

            if let Some((cpu, idx)) = slot {
                system::acpi::GLOBAL_IRQS.lock().insert(
                    gsi_base + i,
                    SpinMutex::new(Box::new(IoApicLine {
                        state: IrqLineState::new(),
                        ioapic: ioapic.clone(),
                        index: i,
                        lapic_id: LAPIC.get_for(cpu).id() as u8,
                        vector: idx as u8,
                        level_triggered: AtomicBool::new(false),
                        active_low: AtomicBool::new(false),
                    })),
                );
            }
        }
    }

    fn read_reg(&self, reg: u32) -> u32 {
        unsafe {
            self.regs.write_reg(ioapic_regs::INDEX, reg).unwrap();
            self.regs.read_reg(ioapic_regs::DATA).unwrap().value()
        }
    }

    fn write_reg(&self, reg: u32, data: u32) {
        unsafe {
            self.regs.write_reg(ioapic_regs::INDEX, reg).unwrap();
            self.regs.write_reg(ioapic_regs::DATA, data).unwrap();
        }
    }
}

pub struct IoApicLine {
    state: IrqLineState,
    ioapic: Arc<IoApic>,
    index: u32,
    // TODO: Where is this used?
    lapic_id: u8,
    vector: u8,
    level_triggered: AtomicBool,
    active_low: AtomicBool,
}

impl IrqLine for IoApicLine {
    fn state(&self) -> &IrqLineState {
        &self.state
    }

    fn set_config(&self, trigger: irq::TriggerMode, polarity: Polarity) -> IrqMode {
        let mode = match trigger {
            irq::TriggerMode::Edge => {
                self.level_triggered.store(false, Ordering::Relaxed);
                IrqMode::EndOfInterrupt | IrqMode::Maskable
            }
            irq::TriggerMode::Level => {
                self.level_triggered.store(true, Ordering::Relaxed);
                IrqMode::EndOfInterrupt | IrqMode::Maskable
            }
        };

        match polarity {
            Polarity::Low => self.active_low.store(true, Ordering::Relaxed),
            Polarity::High => self.active_low.store(false, Ordering::Relaxed),
        }

        mode
    }

    fn mask(&self) {
        self.ioapic.write_reg(
            ioapic_regs::INTS + self.index * 2,
            BitValue::new(0)
                .write_field(ioapic_regs::VECTOR, self.vector)
                .write_field(ioapic_regs::DELIVERY_MODE, 0)
                .write_field(
                    ioapic_regs::LEVEL_TRIGGERED,
                    self.level_triggered.load(Ordering::Relaxed) as u8,
                )
                .write_field(
                    ioapic_regs::ACTIVE_LOW,
                    self.active_low.load(Ordering::Relaxed) as u8,
                )
                .write_field(ioapic_regs::MASKED, true as u8)
                .value(),
        );

        self.ioapic.read_reg(ioapic_regs::INTS + self.index * 2);
    }

    fn unmask(&self) {
        self.ioapic.write_reg(
            ioapic_regs::INTS + self.index * 2,
            BitValue::new(0)
                .write_field(ioapic_regs::VECTOR, self.vector)
                .write_field(ioapic_regs::DELIVERY_MODE, 0)
                .write_field(
                    ioapic_regs::LEVEL_TRIGGERED,
                    self.level_triggered.load(Ordering::Relaxed) as u8,
                )
                .write_field(
                    ioapic_regs::ACTIVE_LOW,
                    self.active_low.load(Ordering::Relaxed) as u8,
                )
                .value(),
        );

        self.ioapic.read_reg(ioapic_regs::INTS + self.index * 2);
    }

    fn end_of_interrupt(&self) {
        LAPIC.get().eoi();
    }
}

const PIC1_COMMAND_PORT: u16 = 0x20;
const PIC1_DATA_PORT: u16 = 0x21;
const PIC2_COMMAND_PORT: u16 = 0xA0;
const PIC2_DATA_PORT: u16 = 0xA1;

/// Masks the legacy Programmable Interrupt Controller so it doesn't get in our way.
pub fn disable_legacy_pic() {
    unsafe {
        // Note: We initialize the PIC properly, but completely disable it and use the APIC in favor of it.
        // Remap IRQs so they start at 0x20 since interrupts 0x00..0x1F are used by CPU exceptions.
        asm::write8(PIC1_COMMAND_PORT, 0x11); // ICW1: Begin initialization and set cascade mode.
        asm::write8(PIC1_DATA_PORT, 0x20); // ICW2: Set where interrupts should be mapped to (0x20-0x27).
        asm::write8(PIC1_DATA_PORT, 0x04); // ICW3: Connect IRQ2 (0x04) to the slave PIC.
        asm::write8(PIC1_DATA_PORT, 0x01); // ICW4: Set the PIC to operate in 8086/88 mode.
        asm::write8(PIC1_DATA_PORT, 0xFF); // Mask all interrupts.

        // Same for the slave PIC.
        asm::write8(PIC2_COMMAND_PORT, 0x11); // ICW1: Begin initialization.
        asm::write8(PIC2_DATA_PORT, 0x28); // ICW2: Set where interrupts should be mapped to (0x28-0x2F).
        asm::write8(PIC2_DATA_PORT, 0x02); // ICW3: Connect to master PIC at IRQ2.
        asm::write8(PIC2_DATA_PORT, 0x01); // ICW4: Set the PIC to operate in 8086/88 mode.
        asm::write8(PIC2_DATA_PORT, 0xFF); // Mask all interrupts.
    }
}
