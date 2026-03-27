//! Early frame buffer boot console (likely using an EFI GOP framebuffer).

use crate::{
    boot::BootInfo,
    memory::{
        PhysAddr, UserPtr, VirtAddr, free, malloc,
        pmm::KernelAlloc,
        virt::{VmFlags, mmu::PageTable},
    },
    posix::errno::{EResult, Errno},
    uapi::{self, termios::winsize},
    vfs::{File, file::FileOps, fs::devtmpfs, inode::Mode},
};
use alloc::sync::Arc;
use core::{
    ffi::{c_char, c_void},
    ptr::null_mut,
};
use flanterm_sys::{flanterm_context, flanterm_get_dimensions, flanterm_write};

#[derive(Default, Debug, Clone)]
pub struct FbColorBits {
    pub offset: u8,
    pub size: u8,
}

#[derive(Debug, Clone)]
pub struct FrameBuffer {
    pub base: PhysAddr,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub cpp: usize,
    pub red: FbColorBits,
    pub green: FbColorBits,
    pub blue: FbColorBits,
}

const FONT_DATA: &[u8] = include_bytes!("../../assets/builtin_font.bin");
const FONT_WIDTH: usize = 8;
const FONT_HEIGHT: usize = 12;

struct FbCon {
    /// Frame buffer that is being drawn on.
    fb: FrameBuffer,
    /// Start of memory mapped region that is used to access the frame buffer.
    mem: *mut u8,
    /// The flanterm context.
    ctx: *mut flanterm_context,
    /// Amount of rows.
    rows: usize,
    /// Amount of columns.
    cols: usize,
}

/// # Safety
/// Pointers are managed by flanterm
unsafe impl Send for FbCon {}
unsafe impl Sync for FbCon {}

impl FbCon {
    pub fn new(fb: FrameBuffer) -> Self {
        // Map the framebuffer in memory.
        let mem = PageTable::get_kernel()
            .map_memory::<KernelAlloc>(
                fb.base,
                VmFlags::Read | VmFlags::Write,
                fb.pitch * fb.height,
            )
            .unwrap();

        log!(
            "Resolution = {}x{}x{}, Phys = {:#018x}, Virt = {:#018x}",
            fb.width,
            fb.height,
            fb.cpp * 8,
            fb.base.value(),
            mem as usize
        );

        unsafe {
            let ctx = flanterm_sys::flanterm_fb_init(
                Some(malloc),
                Some(free),
                mem as *mut u32,
                fb.width,
                fb.height,
                fb.pitch,
                fb.red.size,
                fb.red.offset,
                fb.green.size,
                fb.green.offset,
                fb.blue.size,
                fb.blue.offset,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                FONT_DATA.as_ptr() as *mut c_void,
                FONT_WIDTH,
                FONT_HEIGHT,
                0,
                1,
                1,
                0,
            );

            let mut cols = 0;
            let mut rows = 0;
            flanterm_get_dimensions(ctx, &raw mut cols, &raw mut rows);

            Self {
                fb,
                mem,
                ctx,
                rows,
                cols,
            }
        }
    }
}

impl Drop for FbCon {
    fn drop(&mut self) {
        unsafe { flanterm_sys::flanterm_deinit(self.ctx, Some(free)) };

        PageTable::get_kernel()
            .unmap_range::<KernelAlloc>(self.mem.into(), self.fb.pitch * self.fb.height)
            .unwrap();
    }
}

impl FileOps for FbCon {
    fn read(&self, file: &File, buffer: &mut [u8], offset: u64) -> EResult<isize> {
        let _ = (offset, buffer, file);
        // TODO: Get multiplexed input from event devices.
        Ok(0)
    }

    fn write(&self, _file: &File, buffer: &[u8], _offset: u64) -> EResult<isize> {
        unsafe { flanterm_write(self.ctx, buffer.as_ptr() as *const c_char, buffer.len()) };
        Ok(buffer.len() as _)
    }

    fn ioctl(&self, _file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        match request as _ {
            uapi::ioctls::TIOCGWINSZ => {
                let mut arg = UserPtr::new(arg);
                arg.write(winsize {
                    ws_row: self.rows as _,
                    ws_col: self.cols as _,
                    ..Default::default()
                })
                .ok_or(Errno::EINVAL)?;
            }
            _ => return Err(Errno::ENOSYS),
        }
        Ok(0)
    }
}

#[initgraph::task(
    name = "generic.fbcon",
    depends = [
        crate::memory::MEMORY_STAGE,
        crate::vfs::fs::devtmpfs::DEVTMPFS_STAGE,
    ],
)]
pub fn FBCON_STAGE() {
    let Some(fb) = BootInfo::get().framebuffer.clone() else {
        return;
    };

    let fbcon = FbCon::new(fb);

    devtmpfs::register_device(
        b"fbcon",
        Arc::new(fbcon),
        Mode::from_bits_truncate(0o666),
        false,
    )
    .unwrap();
}
