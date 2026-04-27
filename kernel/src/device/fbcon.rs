//! Early frame buffer boot console (likely using an EFI GOP framebuffer).

use crate::{
    boot::BootInfo,
    device::tty::{Tty, TtyDriver},
    log::{self, LoggerSink},
    memory::{
        PhysAddr, free, malloc,
        pmm::KernelAlloc,
        virt::{VmFlags, mmu::PageTable},
    },
    uapi::termios::winsize,
};
use alloc::{boxed::Box, string::String, sync::Arc};
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

#[derive(Clone)]
struct FbCon {
    pitch: usize,
    height: usize,
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
    pub fn new(fb: &FrameBuffer) -> Self {
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
                pitch: fb.pitch,
                height: fb.height,
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
            .unmap_range::<KernelAlloc>(self.mem.into(), self.pitch * self.height)
            .unwrap();
    }
}

impl TtyDriver for FbCon {
    fn write_output(&self, data: &[u8]) {
        for &byte in data {
            unsafe { flanterm_write(self.ctx, &byte as *const u8 as *const c_char, 1) };
        }
    }

    fn get_winsize(&self) -> winsize {
        winsize {
            ws_row: self.rows as _,
            ws_col: self.cols as _,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }
}

impl LoggerSink for FbCon {
    fn name(&self) -> &'static str {
        "fbcon"
    }

    fn write(&mut self, input: &[u8]) {
        for &byte in input {
            unsafe { flanterm_write(self.ctx, &byte as *const u8 as *const c_char, 1) };
        }
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

    if BootInfo::get()
        .command_line
        .get_bool("fbcon")
        .unwrap_or(true)
    {
        let fbcon = FbCon::new(&fb);
        log::add_sink(Box::new(fbcon.clone()));

        let tty = Tty::new(String::from("fbcon"), Arc::new(fbcon));
        tty.register_device().expect("Unable to create fbcon");
    }
}
