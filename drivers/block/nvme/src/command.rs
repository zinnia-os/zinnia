use crate::{
    error::NvmeError,
    queue::Queue,
    spec::{self},
};
use zinnia::memory::{BitValue, PhysAddr, UnsafeMemoryView};

pub trait Command {
    unsafe fn write_command(&self, view: &impl UnsafeMemoryView) -> Result<(), NvmeError>;
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct ReadWriteCommand {
    pub buffer: PhysAddr,
    pub do_write: bool,
    pub start_lba: u64,
    pub num_lbas: usize,
    pub bytes: usize,
    pub control: u16,
    pub ds_mgmt: u32,
    pub ref_tag: u32,
    pub app_tag: u16,
    pub app_mask: u16,
    pub nsid: u32,
}

impl Command for ReadWriteCommand {
    unsafe fn write_command(&self, view: &impl UnsafeMemoryView) -> Result<(), NvmeError> {
        unsafe {
            let cdw0 = BitValue::new(0)
                .write_field(
                    spec::sq_entry::OPC,
                    if self.do_write {
                        spec::cmd::WRITE
                    } else {
                        spec::cmd::READ
                    },
                )
                .write_field(spec::sq_entry::PSDT, 0); // We always want to use PRP for this.

            let cdw10 =
                BitValue::new(0).write_field(spec::sq_entry::rw::SLBA_LOW, self.start_lba as u32);

            let cdw11 = BitValue::new(0)
                .write_field(spec::sq_entry::rw::SLBA_HIGH, (self.start_lba >> 32) as u32);

            let cdw12 =
                BitValue::new(0).write_field(spec::sq_entry::rw::NLB, (self.num_lbas - 1) as u16);
            let cdw13 = BitValue::new(0);
            let cdw14 = BitValue::new(0);
            let cdw15 = BitValue::new(0);

            view.write_reg(spec::sq_entry::NSID, self.nsid);
            // TODO: For large transfers, we need to setup the PRP in a special way.
            view.write_reg(spec::sq_entry::DPTR0, self.buffer.into());
            view.write_reg(spec::sq_entry::CDW0, cdw0.value());
            view.write_reg(spec::sq_entry::CDW10, cdw10.value());
            view.write_reg(spec::sq_entry::CDW11, cdw11.value());
            view.write_reg(spec::sq_entry::CDW12, cdw12.value());
            view.write_reg(spec::sq_entry::CDW13, cdw13.value());
            view.write_reg(spec::sq_entry::CDW14, cdw14.value());
            view.write_reg(spec::sq_entry::CDW15, cdw15.value());
        }

        Ok(())
    }
}

pub struct CreateCQCommand<'a> {
    pub queue: &'a Queue,
    pub irqs_enabled: bool,
    pub irq_vector: u16,
}

impl Command for CreateCQCommand<'_> {
    unsafe fn write_command(&self, view: &impl UnsafeMemoryView) -> Result<(), NvmeError> {
        unsafe {
            let cdw0 = BitValue::new(0)
                .write_field(spec::sq_entry::OPC, spec::admin_cmd::CREATE_CQ)
                .write_field(spec::sq_entry::PSDT, 0); // We always want to use PRP for this.

            let cdw10 = BitValue::new(0)
                .write_field(
                    spec::sq_entry::create_cq::QSIZE,
                    (self.queue.get_depth() - 1) as u16,
                )
                .write_field(spec::sq_entry::create_cq::QID, self.queue.get_id() as u16);

            let cdw11 = BitValue::new(0)
                .write_field(spec::sq_entry::create_cq::IV, self.irq_vector)
                .write_field(
                    spec::sq_entry::create_cq::IEN,
                    if self.irqs_enabled { 1 } else { 0 },
                ) // TODO: Enable interrupts.
                .write_field(spec::sq_entry::create_cq::PC, 1); // Our buffer is physically contiguous.

            view.write_reg(spec::sq_entry::DPTR0, self.queue.get_cq_addr().into());
            view.write_reg(spec::sq_entry::CDW0, cdw0.value());
            view.write_reg(spec::sq_entry::CDW10, cdw10.value());
            view.write_reg(spec::sq_entry::CDW11, cdw11.value());
        }

        Ok(())
    }
}

pub struct CreateSQCommand<'a> {
    pub queue: &'a Queue,
}

impl Command for CreateSQCommand<'_> {
    unsafe fn write_command(&self, view: &impl UnsafeMemoryView) -> Result<(), NvmeError> {
        unsafe {
            let cdw0 = BitValue::new(0)
                .write_field(spec::sq_entry::OPC, spec::admin_cmd::CREATE_SQ)
                .write_field(spec::sq_entry::PSDT, 0); // We always want to use PRP for this.

            let cdw10 = BitValue::new(0)
                .write_field(
                    spec::sq_entry::create_sq::QSIZE,
                    (self.queue.get_depth() - 1) as u16,
                )
                .write_field(spec::sq_entry::create_sq::QID, self.queue.get_id() as u16);

            let cdw11 = BitValue::new(0)
                .write_field(spec::sq_entry::create_sq::CQID, self.queue.get_id() as u16)
                .write_field(spec::sq_entry::create_sq::QPRIO, 0) // Priority is the highest.
                .write_field(spec::sq_entry::create_sq::PC, 1); // Our buffer is physically contiguous.

            view.write_reg(spec::sq_entry::DPTR0, self.queue.get_sq_addr().into());
            view.write_reg(spec::sq_entry::CDW0, cdw0.value());
            view.write_reg(spec::sq_entry::CDW10, cdw10.value());
            view.write_reg(spec::sq_entry::CDW11, cdw11.value());
        }

        Ok(())
    }
}

pub struct IdentifyCommand {
    pub buffer: PhysAddr,
    pub controller_id: u16,
    pub cns: u8,
    pub nsid: u32,
}

impl Command for IdentifyCommand {
    unsafe fn write_command(&self, view: &impl UnsafeMemoryView) -> Result<(), NvmeError> {
        unsafe {
            let cdw0 = BitValue::new(0)
                .write_field(spec::sq_entry::OPC, spec::admin_cmd::IDENTIFY)
                .write_field(spec::sq_entry::PSDT, 0); // We always want to use PRP for this.

            let cdw10 = BitValue::new(0)
                .write_field(spec::sq_entry::identify::CNTID, self.controller_id)
                .write_field(spec::sq_entry::identify::CNS, self.cns);

            view.write_reg(spec::sq_entry::DPTR0, self.buffer.into());
            view.write_reg(spec::sq_entry::CDW0, cdw0.value());
            view.write_reg(spec::sq_entry::NSID, self.nsid);
            view.write_reg(spec::sq_entry::CDW10, cdw10.value());
        }

        Ok(())
    }
}
