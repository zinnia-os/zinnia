use crate::{arch, irq::lock::IrqLock, memory::PhysAddr, util::mutex::spin::SpinMutex};
use alloc::{boxed::Box, vec::Vec};
use bitflags::bitflags;
use core::{
    fmt::Debug,
    sync::atomic::{AtomicBool, Ordering},
};

pub mod lock;

#[derive(Debug)]
pub enum Status {
    /// Interrupt was not handled (NACK).
    Ignored,
    /// Interrupt was handled (ACK).
    Handled,
}

#[derive(PartialEq, Debug)]
pub enum Polarity {
    Low,
    High,
}

#[derive(PartialEq, Debug)]
pub enum TriggerMode {
    Edge,
    Level,
}

bitflags! {
    /// Describes how to handle an interrupt.
    #[derive(Clone, Copy)]
    pub struct IrqMode: u8 {
        /// Should signal an EOI to the controller.
        const EndOfInterrupt = 1 << 0;
        /// Interrupt can be masked.
        const Maskable = 1 << 1;
    }
}

pub struct IrqConfig {}

pub trait IrqHandler: Send {
    /// Handles an interrupt when it happens.
    fn raise(&mut self) -> Status;
}

pub struct IrqLineState {
    is_busy: AtomicBool,
    handlers: SpinMutex<Vec<Box<dyn IrqHandler>>>,
    mode: SpinMutex<IrqMode>,
}

impl IrqLineState {
    pub const fn new() -> Self {
        Self {
            is_busy: AtomicBool::new(false),
            handlers: SpinMutex::new(Vec::new()),
            mode: SpinMutex::new(IrqMode::empty()),
        }
    }
}

/// Represents an interrupt line of an interrupt controller.
pub trait IrqLine: Sync + Send {
    /// Returns a reference to internal state of this line.
    fn state(&self) -> &IrqLineState;

    /// Changes the current IRQ configuration and returns
    fn set_config(&self, trigger: TriggerMode, polarity: Polarity) -> IrqMode;

    /// Masks this line.
    fn mask(&self);

    /// Unmasks this line.
    fn unmask(&self);

    /// Called to signal the end of an interrupt. Optional.
    fn end_of_interrupt(&self) {}
}

impl dyn IrqLine {
    pub fn program(&self, cfg: Option<(TriggerMode, Polarity)>) {
        let _ = IrqLock::lock();
        let mut mode = self.state().mode.lock();

        if let Some((trigger, polarity)) = cfg {
            *mode = self.set_config(trigger, polarity);
        }
    }

    pub fn raise(&self) {
        assert!(!arch::irq::get_irq_state());

        let _ = IrqLock::lock();
        let state = self.state();

        state.is_busy.store(true, Ordering::Relaxed);
        let mode = *state.mode.lock();

        // TODO
        log!("Handling IRQ");

        let mut claimed = false;
        for handler in state.handlers.lock().iter_mut() {
            match handler.raise() {
                Status::Ignored => (),
                Status::Handled => {
                    claimed = true;
                }
            }
        }

        if !claimed {
            log!("Spurious interrupt");
        }

        if mode.contains(IrqMode::EndOfInterrupt) {
            self.end_of_interrupt();
        }

        state.is_busy.store(false, Ordering::Relaxed);
    }

    /// Attaches a new handler to this line.
    pub fn attach(&self, handler: Box<dyn IrqHandler>) {
        self.state().handlers.lock().push(handler);
    }
}

pub trait MsiLine: IrqLine {
    fn msg_addr(&self) -> PhysAddr;

    fn msg_data(&self) -> u32;
}
