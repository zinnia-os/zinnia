use super::{
    device::fbcon::FrameBuffer,
    memory::VirtAddr,
    util::{mutex::spin::SpinMutex, once::Once},
};
use crate::{cmdline::CmdLine, memory::PhysAddr};

// Boot method selection. Limine is the default method.
#[cfg(all(
    feature = "boot_limine",
    any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64"
    )
))]
pub mod limine;

pub mod initgraph;

/// Information passed from the bootloader. Memory is reclaimed after initialization.
#[derive(Debug)]
pub struct BootInfo {
    /// Kernel command line.
    pub command_line: CmdLine<'static>,
    /// Files given to the bootloader.
    pub files: &'static [BootFile],
    /// Base address for the kernel to access physical memory.
    pub hhdm_address: Option<VirtAddr>,
    /// How many levels the page table has.
    pub paging_level: Option<usize>,
    /// A list of usable physical memory.
    pub memory_map: SpinMutex<&'static mut [PhysMemory]>,
    /// The highest possible physical memory address.
    pub highest_phys: Option<PhysAddr>,
    /// The start of the physical kernel address.
    pub kernel_phys: Option<PhysAddr>,
    /// The start of the virtual kernel address.
    pub kernel_virt: Option<VirtAddr>,
    /// Base address of the RSDT/XSDT ACPI table.
    pub rsdp_addr: Option<PhysAddr>,
    /// Base address of a flattened device tree in memory.
    pub fdt_addr: Option<PhysAddr>,
    /// Early framebuffer if it exists.
    pub framebuffer: Option<FrameBuffer>,
}

static BOOT_INFO: Once<BootInfo> = Once::new();

impl BootInfo {
    pub const fn new() -> Self {
        Self {
            command_line: CmdLine::new(""),
            files: &[],
            hhdm_address: None,
            paging_level: None,
            memory_map: SpinMutex::new(&mut []),
            highest_phys: None,
            kernel_phys: None,
            kernel_virt: None,
            rsdp_addr: None,
            fdt_addr: None,
            framebuffer: None,
        }
    }

    pub fn register(self) {
        unsafe { BOOT_INFO.init(self) };
    }

    pub fn get() -> &'static Self {
        return BOOT_INFO.get();
    }
}

/// A file loaded by the bootloader. Memory may be reclaimed after initialization.
#[derive(Clone, Copy, Debug)]
pub struct BootFile {
    pub data: PhysAddr,
    pub length: usize,
    pub name: &'static str,
}

impl BootFile {
    pub const fn new() -> Self {
        Self {
            data: PhysAddr::null(),
            length: 0,
            name: "",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, PartialOrd)]
pub enum PhysMemoryUsage {
    #[default]
    Reserved,
    Reclaimable,
    Usable,
}

/// Describes a region of physical memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct PhysMemory {
    /// Start address of the memory region.
    pub address: PhysAddr,
    /// Length of the memory region in bytes.
    pub length: usize,
    /// How this memory is used.
    pub usage: PhysMemoryUsage,
}

impl PhysMemory {
    pub const fn empty() -> Self {
        Self {
            address: PhysAddr::null(),
            length: 0,
            usage: PhysMemoryUsage::Reserved,
        }
    }
}

/// Generic entry point.
pub fn entry() {
    #[cfg(all(
        feature = "boot_limine",
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "riscv64",
            target_arch = "loongarch64"
        )
    ))]
    limine::entry();
}
