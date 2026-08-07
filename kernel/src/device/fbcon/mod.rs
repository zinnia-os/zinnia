//! Early frame buffer boot console (likely using an EFI GOP framebuffer).

mod color;
mod parser;
mod tables;
mod term;
mod unicode;

use crate::{
    boot::BootInfo,
    device::vt::{self, VtDisplay},
    log::{self, LoggerSink},
    memory::{
        PhysAddr,
        pmm::KernelAlloc,
        virt::{VmCacheType, VmFlags, mmu::PageTable},
    },
    uapi::termios::winsize,
    util::mutex::spin::SpinMutex,
};
use alloc::{boxed::Box, sync::Arc};
use color::PixelFormat;
use term::{Term, TermConfig};

#[derive(Default, Debug, Clone, Copy)]
pub struct FbColorBits {
    pub offset: u8,
    pub size: u8,
}

#[derive(Debug, Clone, Copy)]
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

const FONT_DATA: &[u8] = include_bytes!("builtin_font.bin");
const FONT_WIDTH: usize = 8;
const FONT_HEIGHT: usize = 12;

struct FbConInner {
    term: SpinMutex<Term>,
    /// Start of the memory mapped region used to access the frame buffer.
    mem: *mut u8,
    map_len: usize,
    rows: usize,
    cols: usize,
}

/// # Safety
/// The framebuffer mapping is guarded by the terminal's lock.
unsafe impl Send for FbConInner {}
unsafe impl Sync for FbConInner {}

impl Drop for FbConInner {
    fn drop(&mut self) {
        PageTable::get_kernel()
            .unmap_memory::<KernelAlloc>(self.mem.into(), self.map_len)
            .unwrap();
    }
}

#[derive(Clone)]
struct FbCon {
    inner: Arc<FbConInner>,
}

impl FbCon {
    fn new(fb: &FrameBuffer) -> Self {
        let map_len = fb.pitch * fb.height;
        let mem = PageTable::get_kernel()
            .map_memory::<KernelAlloc>(
                fb.base,
                VmFlags::Read | VmFlags::Write,
                VmCacheType::WriteCombine,
                map_len,
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

        let term = Term::new(TermConfig {
            fb: mem as *mut u32,
            width: fb.width,
            height: fb.height,
            pitch: fb.pitch,
            format: PixelFormat::new(
                fb.red.size,
                fb.red.offset,
                fb.green.size,
                fb.green.offset,
                fb.blue.size,
                fb.blue.offset,
            ),
            font: FONT_DATA,
            font_width: FONT_WIDTH,
            font_height: FONT_HEIGHT,
        });

        let rows = term.rows();
        let cols = term.cols();

        Self {
            inner: Arc::new(FbConInner {
                term: SpinMutex::new(term),
                mem,
                map_len,
                rows,
                cols,
            }),
        }
    }

    fn write_bytes(&self, data: &[u8]) {
        let mut term = self.inner.term.lock();
        for &byte in data {
            if byte == b'\n' {
                term.putchar(b'\r');
            }
            term.putchar(byte);
        }
        term.flush();
    }
}

impl VtDisplay for FbCon {
    fn write_output(&self, data: &[u8]) {
        self.write_bytes(data);
    }

    fn get_winsize(&self) -> winsize {
        winsize {
            ws_row: self.inner.rows as _,
            ws_col: self.inner.cols as _,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }

    fn refresh(&self) {
        self.inner.term.lock().full_refresh();
    }
}

impl LoggerSink for FbCon {
    fn name(&self) -> &'static str {
        "fbcon"
    }

    fn write(&mut self, input: &[u8]) {
        self.write_bytes(input);
    }
}

#[task(
    name = "generic.fbcon",
    depends = [
        crate::memory::MEMORY_STAGE,
        crate::vfs::fs::devtmpfs::DEVTMPFS_STAGE,
        crate::device::vt::VT_STAGE,
    ],
)]
pub fn FBCON_STAGE() {
    let info = BootInfo::get();
    let Some(fb) = info.framebuffer else {
        return;
    };

    if !info.command_line.get_bool("fbcon").unwrap_or(true) {
        return;
    }

    let fbcon = FbCon::new(&fb);
    log::add_sink(Box::new(fbcon.clone()));
    vt::attach_display(Arc::new(fbcon));
}
