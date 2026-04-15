#![no_std]
#![feature(negative_impls)]
#![feature(allocator_api)]
#![feature(str_from_raw_parts)]
#![feature(likely_unlikely)]
#![feature(slice_split_once)]
#![feature(bool_to_result)]
#![feature(box_into_inner)]
#![feature(const_index)]
#![feature(const_trait_impl)]
#![feature(const_cmp)]
#![feature(macro_attr)]
#![feature(macro_metavar_expr)]
// Clippy lints
#![allow(clippy::needless_return)]
#![allow(clippy::new_without_default)]
#![forbid(clippy::missing_safety_doc)]
#![forbid(clippy::large_stack_frames)]

#[macro_use]
pub extern crate alloc;
#[macro_use]
pub extern crate core;

#[macro_use]
pub mod macros;
pub mod arch;
pub mod boot;
pub mod clock;
pub mod cmdline;
pub mod device;
pub mod init;
pub mod irq;
pub mod log;
pub mod memory;
pub mod module;
pub mod panic;
pub mod percpu;
pub mod posix;
pub mod process;
pub mod sched;
pub mod syscall;
pub mod system;
pub mod uapi;
pub mod util;
pub mod vfs;

use crate::{
    irq::lock::IrqLock,
    percpu::CpuData,
    process::{Identity, Process},
    util::once::Once,
    vfs::{
        File,
        file::{FileDescription, OpenFlags},
        fs::{devtmpfs, initramfs},
        inode::Mode,
    },
};
use alloc::{string::String, sync::Arc};
use boot::BootInfo;
use core::sync::atomic::AtomicBool;

/// Initializes all important kernel structures.
/// This is invoked by the boot environment.
pub fn init() -> ! {
    {
        let _irq = IrqLock::lock();
        init::run();
    }

    CpuData::get().scheduler.do_yield();
    unreachable!("The scheduler got back to zinnia::init?");
}

pub(crate) static INIT: Once<Arc<Process>> = Once::new();

/// The high-level kernel entry point.
pub extern "C" fn main(_: usize, _: usize) {
    // Say hello to the console.
    log!("Zinnia {}", env!("CARGO_PKG_VERSION"));
    log!("Command line: {}", BootInfo::get().command_line.inner());

    // Load all initramfs files.
    {
        let kernel_proc = Process::get_kernel();
        let root_dir = kernel_proc.root_dir.lock().clone();
        for file in BootInfo::get().files {
            initramfs::load(root_dir.clone(), root_dir.clone(), unsafe {
                core::slice::from_raw_parts(file.data.as_hhdm(), file.length)
            })
            .expect("Failed to load one of the provided initramfs archives");
        }
    }

    // Find user space init. If no path is given, search a few select directories.
    let path = match BootInfo::get().command_line.get_string("init") {
        Some(x) => x.as_bytes(),
        // TODO: Search VFS for alternative init paths like "/sbin/init"
        None => b"/init",
    };

    log!("Starting init \"{}\"", String::from_utf8_lossy(path));

    let args = vec![path.to_vec()];
    log!("With arguments:");
    args.iter()
        .for_each(|x| log!("    {}", String::from_utf8_lossy(x)));

    let envs = vec![b"HOME=/".to_vec(), b"TERM=xterm-256color".to_vec()];
    log!("With environment:");
    envs.iter()
        .for_each(|x| log!("    {}", String::from_utf8_lossy(x)));

    unsafe {
        INIT.init(Arc::new(
            Process::new("init".into(), Some(Process::get_kernel().clone()))
                .expect("Unable to create init process"),
        ))
    };

    let init_proc = INIT.get();
    Process::get_kernel()
        .children
        .lock()
        .push(init_proc.clone());
    process::PROCESS_TABLE
        .lock()
        .insert(init_proc.get_pid(), Arc::downgrade(&init_proc));

    let tty = match BootInfo::get().command_line.get_string("console") {
        Some(x) => device::tty::get_tty_by_name(x),
        None => device::tty::get_tty(0),
    }
    .expect("Unable to open TTY for init");

    {
        *tty.session.lock() = Some(init_proc.get_pid());
        *tty.foreground_pgrp.lock() = Some(*init_proc.pgrp.lock());
        *init_proc.controlling_tty.lock() = Some(tty);
    }

    // Open tty for stdio for init.
    {
        let mut open_files = init_proc.open_files.lock();
        let dev = devtmpfs::get_root();
        let console = File::open(
            dev.clone(),
            dev,
            BootInfo::get()
                .command_line
                .get_string("console")
                .unwrap_or("com1")
                .as_bytes(),
            OpenFlags::ReadWrite,
            Mode::empty(),
            Identity::get_kernel(),
        )
        .expect("Unable to open console for init");

        for i in 0..=2 {
            open_files.open_file(
                FileDescription {
                    file: console.clone(),
                    close_on_exec: AtomicBool::new(false),
                },
                i,
            );
        }
    }

    let init_file = {
        File::open(
            init_proc.root_dir.lock().clone(),
            init_proc.working_dir.lock().clone(),
            path,
            OpenFlags::Read | OpenFlags::Executable,
            Mode::empty(),
            Identity::get_kernel(),
        )
        .expect("Unable to read the init executable")
    };

    init_proc
        .clone()
        .fexecve(init_file, args, envs)
        .expect("Unable to start the init process");
}
