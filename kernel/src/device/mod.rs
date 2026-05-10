#[cfg(all(
    any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64"
    ),
    feature = "acpi"
))]
pub mod acpi;
pub mod block;
pub mod cmdline;
pub mod drm;
pub mod dt;
pub mod fbcon;
pub mod input;
pub mod kmsg;
pub mod memfiles;
pub mod net;
pub mod pci;
pub mod tty;
pub mod vt;
