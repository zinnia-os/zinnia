//! Boot using the Limine protocol.

use super::{BootFile, BootInfo, PhysMemory};
use crate::{
    cmdline::CmdLine,
    device::fbcon::{FbColorBits, FrameBuffer},
    util::mutex::spin::SpinMutex,
};
use limine::{BaseRevision, memory_map::EntryType, paging::Mode, request::*};

#[used]
#[unsafe(link_section = ".boot.init")]
pub static START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".boot")]
pub static BASE_REVISION: BaseRevision = BaseRevision::with_revision(6);

#[used]
#[unsafe(link_section = ".boot.fini")]
pub static END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

#[unsafe(link_section = ".boot")]
pub static BOOTLOADER_REQUEST: BootloaderInfoRequest = BootloaderInfoRequest::new();

#[unsafe(link_section = ".boot")]
pub static MEMMAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[unsafe(link_section = ".boot")]
pub static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[unsafe(link_section = ".boot")]
pub static PAGING_REQUEST: PagingModeRequest = PagingModeRequest::new();

#[unsafe(link_section = ".boot")]
pub static KERNEL_ADDR_REQUEST: ExecutableAddressRequest = ExecutableAddressRequest::new();

#[unsafe(link_section = ".boot")]
pub static COMMAND_LINE_REQUEST: ExecutableCmdlineRequest = ExecutableCmdlineRequest::new();

#[unsafe(link_section = ".boot")]
pub static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[unsafe(link_section = ".boot")]
pub static MODULE_REQUEST: ModuleRequest = ModuleRequest::new();

#[unsafe(link_section = ".boot")]
pub static RSDP_REQUEST: RsdpRequest = RsdpRequest::new();

#[unsafe(link_section = ".boot")]
pub static DTB_REQUEST: DeviceTreeBlobRequest = DeviceTreeBlobRequest::new();

static mut MEMMAP_BUF: [PhysMemory; 128] = [PhysMemory::empty(); _];
static mut FILE_BUF: [BootFile; 32] = [BootFile::new(); _];

const STRING_BUF_LEN: usize = 2048;
static mut CMDLINE_BUF: [u8; STRING_BUF_LEN] = [0; _];
static mut FILE_NAME_BUF: [u8; STRING_BUF_LEN] = [0; _];

pub fn entry() -> ! {
    crate::arch::cpu::setup_bsp();

    // Start collecting boot info.
    let mut info = BootInfo::new();

    {
        // Convert the memory map. This buffer has to be fixed since at this point
        // in the boot process since there is no dynamic memory allocator available yet.
        // 128 entries should be enough for all use cases.
        let entries = MEMMAP_REQUEST.get_response().unwrap().entries();
        let mut total_entries = 0;
        entries.iter().enumerate().for_each(|(i, entry)| unsafe {
            MEMMAP_BUF[i] = PhysMemory {
                length: entry.length as usize,
                address: entry.base.into(),
                usage: match entry.entry_type {
                    EntryType::USABLE => super::PhysMemoryUsage::Usable,
                    EntryType::BOOTLOADER_RECLAIMABLE | EntryType::EXECUTABLE_AND_MODULES => {
                        super::PhysMemoryUsage::Reclaimable
                    }
                    _ => super::PhysMemoryUsage::Reserved,
                },
            };
            total_entries += 1;
        });

        info.highest_phys = Some({
            let last = entries.iter().last().unwrap();
            (last.base + last.length).into()
        });

        // Get kernel physical and virtual base.
        let kernel_addr = KERNEL_ADDR_REQUEST.get_response().unwrap();

        info.hhdm_address = Some(HHDM_REQUEST.get_response().unwrap().offset().into());

        let paging = PAGING_REQUEST.get_response().unwrap().mode();
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        {
            if paging == Mode::FOUR_LEVEL {
                info.paging_level = Some(4);
            } else if paging == Mode::FIVE_LEVEL {
                info.paging_level = Some(5);
            }
        }
        #[cfg(target_arch = "riscv64")]
        {
            if paging == Mode::SV39 {
                info.paging_level = Some(3);
            } else if paging == Mode::SV48 {
                info.paging_level = Some(4);
            } else if paging == Mode::SV57 {
                info.paging_level = Some(5);
            }
        }
        #[cfg(target_arch = "loongarch64")]
        {
            if paging == Mode::FOUR_LEVEL {
                info.paging_level = Some(4);
            }
        }

        unsafe {
            info.memory_map = SpinMutex::new(&mut MEMMAP_BUF[0..total_entries]);
        }
        info.kernel_phys = Some(kernel_addr.physical_base().into());
        info.kernel_virt = Some(kernel_addr.virtual_base().into());
    }

    // Convert the command line from bytes to UTF-8 if there is any.
    info.command_line = unsafe {
        let line = COMMAND_LINE_REQUEST.get_response().unwrap().cmdline();
        let len = line.count_bytes().min(STRING_BUF_LEN);
        let buf = &raw mut CMDLINE_BUF as *mut u8;
        core::ptr::copy_nonoverlapping(line.as_ptr().cast(), buf, len);
        CmdLine::new(str::from_utf8(core::slice::from_raw_parts(buf, len)).unwrap_or_default())
    };

    info.rsdp_addr = RSDP_REQUEST
        .get_response()
        .map(|x| (x.address() - info.hhdm_address.unwrap().value()).into());

    // The FDT is a virtual address.
    info.fdt_addr = DTB_REQUEST.get_response().map(|x| {
        unsafe {
            x.dtb_ptr()
                .byte_sub(HHDM_REQUEST.get_response().unwrap().offset() as usize)
        }
        .into()
    });

    // Get all modules.
    if let Some(response) = MODULE_REQUEST.get_response() {
        let mut name_offset = 0;
        for (i, entry) in response.modules().iter().enumerate() {
            unsafe {
                // Split off any parts of the path that come before the actual file name.
                let name = entry.path().to_str().unwrap().rsplit_once('/').unwrap().1;
                assert!(name_offset + name.len() <= STRING_BUF_LEN);
                let copied = (&raw mut FILE_NAME_BUF as *mut u8).add(name_offset);
                core::ptr::copy_nonoverlapping(name.as_ptr(), copied, name.len());
                name_offset += name.len();

                FILE_BUF[i] = BootFile {
                    // We need files to be in physical form.
                    data: (entry.addr() as usize)
                        .wrapping_sub(info.hhdm_address.unwrap().value())
                        .into(),
                    length: entry.size() as usize,
                    name: str::from_utf8_unchecked(core::slice::from_raw_parts(copied, name.len())),
                }
            };
        }
        unsafe {
            info.files = &FILE_BUF[0..response.modules().len()];
        }
    }

    if let Some(response) = FRAMEBUFFER_REQUEST.get_response()
        && let Some(fb) = response.framebuffers().next()
    {
        // We can't call `as_hhdm` yet because it's not been initialized yet.
        let fb_addr = fb.addr() as usize;
        let hhdm = (HHDM_REQUEST.get_response().unwrap().offset()) as usize;

        info.framebuffer = Some(FrameBuffer {
            base: (fb_addr - hhdm).into(),
            width: fb.width() as usize,
            height: fb.height() as usize,
            pitch: fb.pitch() as usize,
            cpp: fb.bpp() as usize / 8,
            red: FbColorBits {
                offset: fb.red_mask_shift(),
                size: fb.red_mask_size(),
            },
            green: FbColorBits {
                offset: fb.green_mask_shift(),
                size: fb.green_mask_size(),
            },
            blue: FbColorBits {
                offset: fb.blue_mask_shift(),
                size: fb.blue_mask_size(),
            },
        });
    }

    // Finally, save the boot information.
    info.register();

    crate::init();
}
