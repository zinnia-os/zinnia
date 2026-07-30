use crate::{dma::DmaRegion, error::GpuError, spec};
use zinnia::{
    alloc::vec::Vec,
    memory::{MemoryView, PhysAddr},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn sized(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Box3d {
    pub x: u32,
    pub y: u32,
    pub z: u32,
    pub w: u32,
    pub h: u32,
    pub d: u32,
}

pub trait Command {
    fn command_type(&self) -> u32;

    fn body_len(&self) -> usize;

    fn encode_body(&self, dst: &mut [u8]) -> Option<()>;

    fn payload(&self) -> Option<&DmaRegion> {
        None
    }

    fn response_len(&self) -> usize {
        spec::ctrl_hdr::SIZE
    }
}

fn write_rect(dst: &mut [u8], offset: usize, value: &Rect) -> Option<()> {
    let dst = dst.get_mut(offset..offset + spec::rect::SIZE)?;
    dst.write_reg(spec::rect::X, value.x)?;
    dst.write_reg(spec::rect::Y, value.y)?;
    dst.write_reg(spec::rect::WIDTH, value.width)?;
    dst.write_reg(spec::rect::HEIGHT, value.height)
}

fn write_box(dst: &mut [u8], offset: usize, value: &Box3d) -> Option<()> {
    let dst = dst.get_mut(offset..offset + spec::box3d::SIZE)?;
    dst.write_reg(spec::box3d::X, value.x)?;
    dst.write_reg(spec::box3d::Y, value.y)?;
    dst.write_reg(spec::box3d::Z, value.z)?;
    dst.write_reg(spec::box3d::W, value.w)?;
    dst.write_reg(spec::box3d::H, value.h)?;
    dst.write_reg(spec::box3d::D, value.d)
}

pub struct GetDisplayInfo;

impl Command for GetDisplayInfo {
    fn command_type(&self) -> u32 {
        spec::cmd::GET_DISPLAY_INFO
    }

    fn body_len(&self) -> usize {
        spec::ctrl_hdr::SIZE
    }

    fn encode_body(&self, _dst: &mut [u8]) -> Option<()> {
        Some(())
    }

    fn response_len(&self) -> usize {
        spec::resp_display_info::SIZE
    }
}

pub struct CreateResource2d {
    pub resource_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

impl Command for CreateResource2d {
    fn command_type(&self) -> u32 {
        spec::cmd::RESOURCE_CREATE_2D
    }

    fn body_len(&self) -> usize {
        spec::resource_create_2d::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        dst.write_reg(spec::resource_create_2d::RESOURCE_ID, self.resource_id)?;
        dst.write_reg(spec::resource_create_2d::FORMAT, self.format)?;
        dst.write_reg(spec::resource_create_2d::WIDTH, self.width)?;
        dst.write_reg(spec::resource_create_2d::HEIGHT, self.height)
    }
}

pub struct CreateResource3d {
    pub resource_id: u32,
    pub target: u32,
    pub format: u32,
    pub bind: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub array_size: u32,
    pub last_level: u32,
    pub nr_samples: u32,
    pub flags: u32,
}

impl Command for CreateResource3d {
    fn command_type(&self) -> u32 {
        spec::cmd::RESOURCE_CREATE_3D
    }

    fn body_len(&self) -> usize {
        spec::resource_create_3d::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        dst.write_reg(spec::resource_create_3d::RESOURCE_ID, self.resource_id)?;
        dst.write_reg(spec::resource_create_3d::TARGET, self.target)?;
        dst.write_reg(spec::resource_create_3d::FORMAT, self.format)?;
        dst.write_reg(spec::resource_create_3d::BIND, self.bind)?;
        dst.write_reg(spec::resource_create_3d::WIDTH, self.width)?;
        dst.write_reg(spec::resource_create_3d::HEIGHT, self.height)?;
        dst.write_reg(spec::resource_create_3d::DEPTH, self.depth)?;
        dst.write_reg(spec::resource_create_3d::ARRAY_SIZE, self.array_size)?;
        dst.write_reg(spec::resource_create_3d::LAST_LEVEL, self.last_level)?;
        dst.write_reg(spec::resource_create_3d::NR_SAMPLES, self.nr_samples)?;
        dst.write_reg(spec::resource_create_3d::FLAGS, self.flags)
    }
}

pub struct UnrefResource {
    pub resource_id: u32,
}

impl Command for UnrefResource {
    fn command_type(&self) -> u32 {
        spec::cmd::RESOURCE_UNREF
    }

    fn body_len(&self) -> usize {
        spec::resource_unref::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        dst.write_reg(spec::resource_unref::RESOURCE_ID, self.resource_id)
    }
}

pub struct SetScanout {
    pub scanout_id: u32,
    pub resource_id: u32,
    pub rect: Rect,
}

impl Command for SetScanout {
    fn command_type(&self) -> u32 {
        spec::cmd::SET_SCANOUT
    }

    fn body_len(&self) -> usize {
        spec::set_scanout::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        write_rect(dst, spec::set_scanout::RECT, &self.rect)?;
        dst.write_reg(spec::set_scanout::SCANOUT_ID, self.scanout_id)?;
        dst.write_reg(spec::set_scanout::RESOURCE_ID, self.resource_id)
    }
}

pub struct FlushResource {
    pub resource_id: u32,
    pub rect: Rect,
}

impl Command for FlushResource {
    fn command_type(&self) -> u32 {
        spec::cmd::RESOURCE_FLUSH
    }

    fn body_len(&self) -> usize {
        spec::resource_flush::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        write_rect(dst, spec::resource_flush::RECT, &self.rect)?;
        dst.write_reg(spec::resource_flush::RESOURCE_ID, self.resource_id)
    }
}

pub struct TransferToHost2d {
    pub resource_id: u32,
    pub rect: Rect,
    pub offset: u64,
}

impl Command for TransferToHost2d {
    fn command_type(&self) -> u32 {
        spec::cmd::TRANSFER_TO_HOST_2D
    }

    fn body_len(&self) -> usize {
        spec::transfer_host_2d::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        write_rect(dst, spec::transfer_host_2d::RECT, &self.rect)?;
        dst.write_reg(spec::transfer_host_2d::OFFSET, self.offset)?;
        dst.write_reg(spec::transfer_host_2d::RESOURCE_ID, self.resource_id)
    }
}

pub struct TransferHost3d {
    pub to_host: bool,
    pub resource_id: u32,
    pub area: Box3d,
    pub offset: u64,
    pub level: u32,
    pub stride: u32,
    pub layer_stride: u32,
}

impl Command for TransferHost3d {
    fn command_type(&self) -> u32 {
        if self.to_host {
            spec::cmd::TRANSFER_TO_HOST_3D
        } else {
            spec::cmd::TRANSFER_FROM_HOST_3D
        }
    }

    fn body_len(&self) -> usize {
        spec::transfer_host_3d::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        write_box(dst, spec::transfer_host_3d::BOX, &self.area)?;
        dst.write_reg(spec::transfer_host_3d::OFFSET, self.offset)?;
        dst.write_reg(spec::transfer_host_3d::RESOURCE_ID, self.resource_id)?;
        dst.write_reg(spec::transfer_host_3d::LEVEL, self.level)?;
        dst.write_reg(spec::transfer_host_3d::STRIDE, self.stride)?;
        dst.write_reg(spec::transfer_host_3d::LAYER_STRIDE, self.layer_stride)
    }
}

pub struct AttachBacking {
    resource_id: u32,
    entries: usize,
    payload: DmaRegion,
}

impl AttachBacking {
    pub fn new(resource_id: u32, runs: &[(PhysAddr, usize)]) -> Result<Self, GpuError> {
        let mut payload = DmaRegion::new(runs.len() * spec::mem_entry::SIZE)?;
        let dst = payload.as_mut_slice();

        for (index, &(addr, length)) in runs.iter().enumerate() {
            let offset = index * spec::mem_entry::SIZE;
            let entry = dst
                .get_mut(offset..offset + spec::mem_entry::SIZE)
                .ok_or(GpuError::EncodingFailed)?;
            entry
                .write_reg(spec::mem_entry::ADDR, addr.value() as u64)
                .ok_or(GpuError::EncodingFailed)?;
            entry
                .write_reg(spec::mem_entry::LENGTH, length as u32)
                .ok_or(GpuError::EncodingFailed)?;
        }

        Ok(Self {
            resource_id,
            entries: runs.len(),
            payload,
        })
    }
}

impl Command for AttachBacking {
    fn command_type(&self) -> u32 {
        spec::cmd::RESOURCE_ATTACH_BACKING
    }

    fn body_len(&self) -> usize {
        spec::attach_backing::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        dst.write_reg(spec::attach_backing::RESOURCE_ID, self.resource_id)?;
        dst.write_reg(spec::attach_backing::NR_ENTRIES, self.entries as u32)
    }

    fn payload(&self) -> Option<&DmaRegion> {
        Some(&self.payload)
    }
}

pub struct DetachBacking {
    pub resource_id: u32,
}

impl Command for DetachBacking {
    fn command_type(&self) -> u32 {
        spec::cmd::RESOURCE_DETACH_BACKING
    }

    fn body_len(&self) -> usize {
        spec::detach_backing::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        dst.write_reg(spec::detach_backing::RESOURCE_ID, self.resource_id)
    }
}

pub struct GetCapsetInfo {
    pub index: u32,
}

impl Command for GetCapsetInfo {
    fn command_type(&self) -> u32 {
        spec::cmd::GET_CAPSET_INFO
    }

    fn body_len(&self) -> usize {
        spec::get_capset_info::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        dst.write_reg(spec::get_capset_info::CAPSET_INDEX, self.index)
    }

    fn response_len(&self) -> usize {
        spec::resp_capset_info::SIZE
    }
}

pub struct GetCapset {
    pub capset_id: u32,
    pub version: u32,
    pub max_size: usize,
}

impl Command for GetCapset {
    fn command_type(&self) -> u32 {
        spec::cmd::GET_CAPSET
    }

    fn body_len(&self) -> usize {
        spec::get_capset::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        dst.write_reg(spec::get_capset::CAPSET_ID, self.capset_id)?;
        dst.write_reg(spec::get_capset::CAPSET_VERSION, self.version)
    }

    fn response_len(&self) -> usize {
        spec::resp_capset::DATA + self.max_size
    }
}

pub struct CreateContext {
    pub name: [u8; spec::ctx_create::DEBUG_NAME_LEN],
    pub name_len: usize,
}

impl CreateContext {
    pub fn new(name: &str) -> Self {
        let mut buffer = [0u8; spec::ctx_create::DEBUG_NAME_LEN];
        let len = name.len().min(buffer.len());
        buffer[..len].copy_from_slice(&name.as_bytes()[..len]);
        Self {
            name: buffer,
            name_len: len,
        }
    }
}

impl Command for CreateContext {
    fn command_type(&self) -> u32 {
        spec::cmd::CTX_CREATE
    }

    fn body_len(&self) -> usize {
        spec::ctx_create::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        dst.write_reg(spec::ctx_create::NLEN, self.name_len as u32)?;
        dst.write_reg(spec::ctx_create::CONTEXT_INIT, 0u32)?;
        let name = dst.get_mut(
            spec::ctx_create::DEBUG_NAME
                ..spec::ctx_create::DEBUG_NAME + spec::ctx_create::DEBUG_NAME_LEN,
        )?;
        name.copy_from_slice(&self.name);
        Some(())
    }
}

pub struct DestroyContext;

impl Command for DestroyContext {
    fn command_type(&self) -> u32 {
        spec::cmd::CTX_DESTROY
    }

    fn body_len(&self) -> usize {
        spec::ctx_destroy::SIZE
    }

    fn encode_body(&self, _dst: &mut [u8]) -> Option<()> {
        Some(())
    }
}

pub struct ContextResource {
    pub attach: bool,
    pub resource_id: u32,
}

impl Command for ContextResource {
    fn command_type(&self) -> u32 {
        if self.attach {
            spec::cmd::CTX_ATTACH_RESOURCE
        } else {
            spec::cmd::CTX_DETACH_RESOURCE
        }
    }

    fn body_len(&self) -> usize {
        spec::ctx_resource::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        dst.write_reg(spec::ctx_resource::RESOURCE_ID, self.resource_id)
    }
}

pub struct Submit3d {
    stream: DmaRegion,
}

impl Submit3d {
    pub fn new(stream: DmaRegion) -> Self {
        Self { stream }
    }
}

impl Command for Submit3d {
    fn command_type(&self) -> u32 {
        spec::cmd::SUBMIT_3D
    }

    fn body_len(&self) -> usize {
        spec::submit_3d::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        dst.write_reg(spec::submit_3d::STREAM_SIZE, self.stream.len() as u32)
    }

    fn payload(&self) -> Option<&DmaRegion> {
        Some(&self.stream)
    }
}

pub struct UpdateCursor {
    pub move_only: bool,
    pub scanout_id: u32,
    pub resource_id: u32,
    pub x: i32,
    pub y: i32,
    pub hot_x: u32,
    pub hot_y: u32,
}

impl Command for UpdateCursor {
    fn command_type(&self) -> u32 {
        if self.move_only {
            spec::cmd::MOVE_CURSOR
        } else {
            spec::cmd::UPDATE_CURSOR
        }
    }

    fn body_len(&self) -> usize {
        spec::update_cursor::SIZE
    }

    fn encode_body(&self, dst: &mut [u8]) -> Option<()> {
        dst.write_reg(spec::update_cursor::POS_SCANOUT_ID, self.scanout_id)?;
        dst.write_reg(spec::update_cursor::POS_X, self.x as u32)?;
        dst.write_reg(spec::update_cursor::POS_Y, self.y as u32)?;
        dst.write_reg(spec::update_cursor::RESOURCE_ID, self.resource_id)?;
        dst.write_reg(spec::update_cursor::HOT_X, self.hot_x)?;
        dst.write_reg(spec::update_cursor::HOT_Y, self.hot_y)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScanoutMode {
    pub id: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct CapsetInfo {
    pub id: u32,
    pub max_version: u32,
    pub max_size: u32,
}

pub fn decode_display_info(response: &[u8]) -> Result<Vec<ScanoutMode>, GpuError> {
    let mut modes = Vec::new();

    for index in 0..spec::MAX_SCANOUTS {
        let offset = spec::resp_display_info::PMODES + index * spec::display_one::SIZE;
        let entry = response
            .get(offset..offset + spec::display_one::SIZE)
            .ok_or(GpuError::ShortResponse)?;

        let enabled = entry
            .read_reg(spec::display_one::ENABLED)
            .ok_or(GpuError::ShortResponse)?
            .value();
        if enabled == 0 {
            continue;
        }

        let area = entry
            .get(spec::display_one::RECT..)
            .ok_or(GpuError::ShortResponse)?;
        modes.push(ScanoutMode {
            id: index as u32,
            width: area
                .read_reg(spec::rect::WIDTH)
                .ok_or(GpuError::ShortResponse)?
                .value(),
            height: area
                .read_reg(spec::rect::HEIGHT)
                .ok_or(GpuError::ShortResponse)?
                .value(),
        });
    }

    if modes.is_empty() {
        return Err(GpuError::NoScanouts);
    }

    Ok(modes)
}

pub fn decode_capset_info(response: &[u8]) -> Result<CapsetInfo, GpuError> {
    Ok(CapsetInfo {
        id: response
            .read_reg(spec::resp_capset_info::CAPSET_ID)
            .ok_or(GpuError::ShortResponse)?
            .value(),
        max_version: response
            .read_reg(spec::resp_capset_info::CAPSET_MAX_VERSION)
            .ok_or(GpuError::ShortResponse)?
            .value(),
        max_size: response
            .read_reg(spec::resp_capset_info::CAPSET_MAX_SIZE)
            .ok_or(GpuError::ShortResponse)?
            .value(),
    })
}
