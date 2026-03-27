//! The initramfs (initial RAM file system) is a CPIO archive which is loaded into memory by the bootloader.
//!
//! This allows the kernel to load drivers needed in order to boot from a block device.
//! It also usually contains the init process which is responsible for actually loading the modules
//! and mounting the real root file system from disk.

use crate::{
    boot::BootInfo,
    module,
    posix::errno::{EResult, Errno},
    process::Identity,
    uapi::time::timespec,
    util::{self},
    vfs::{
        self, PathNode,
        file::{File, OpenFlags},
        inode::Mode,
    },
};
use bytemuck::AnyBitPattern;
use core::ffi::CStr;

#[repr(C)]
#[derive(AnyBitPattern, Clone, Copy)]
struct FileHeader {
    name: [u8; 100],
    mode: [u8; 8],
    uid: [u8; 8],
    gid: [u8; 8],
    size: [u8; 12],
    mtime: [u8; 12],
    checksum: [u8; 8],
    typ: u8,
    linkname: [u8; 100],
    signature: [u8; 6],
    version: [u8; 2],
    owner: [u8; 32],
    group: [u8; 32],
    devmajor: [u8; 8],
    devminor: [u8; 8],
    prefix: [u8; 155],
    pad: [u8; 12],
}
static_assert!(size_of::<FileHeader>() == 512);

const REGULAR: u8 = 0;
const NORMAL: u8 = b'0';
const SYM_LINK: u8 = b'2';
const DIRECTORY: u8 = b'5';
const CONTIGOUS: u8 = b'7';
const LONG_LINK: u8 = b'L';

/// Converts a 0-terminated octal string into a number.
fn oct2bin(str: &[u8]) -> usize {
    let mut n = 0;
    for s in str {
        if *s == 0 || *s == b' ' {
            break;
        }
        n *= 8;
        n += (*s - b'0') as usize;
    }
    return n;
}

/// Creates all directories in the given path and opens the last one. Also returns the final file name.
pub fn create_dirs<'a>(
    root: PathNode,
    at: PathNode,
    path: &'a [u8],
    mode: Mode,
) -> EResult<(PathNode, &'a [u8])> {
    let mut current = at;

    // If there is a path to split off, do that. If there isn't, there are no directories to create.
    let (path, file_name) = match path.rsplit_once(|&x| x == b'/') {
        Some(x) => x,
        None => (&[][..], path),
    };

    for component in path.split(|&x| x == b'/').filter(|&x| !x.is_empty()) {
        if let Err(e) = vfs::mkdir(
            root.clone(),
            current.clone(),
            component,
            mode.clone(),
            Identity::get_kernel(),
        ) && e != Errno::EEXIST
        {
            return Err(e);
        }

        current = File::open(
            root.clone(),
            current.clone(),
            component,
            OpenFlags::Directory,
            Mode::empty(),
            Identity::get_kernel(),
        )?
        .path
        .as_ref()
        .unwrap()
        .clone();
    }

    return Ok((current, file_name));
}

pub fn load(root: PathNode, target: PathNode, data: &[u8]) -> EResult<()> {
    let mut offset = 0;
    let mut name_override = None;
    let mut files_loaded = 0;

    loop {
        let current_file: &FileHeader =
            bytemuck::try_from_bytes(&data[offset..][..size_of::<FileHeader>()]).unwrap();
        if &current_file.signature[0..5] != b"ustar" {
            break;
        }

        let file_mode = Mode::from_bits_truncate(oct2bin(&current_file.mode) as _);
        let file_size = oct2bin(&current_file.size);
        let mtime = oct2bin(&current_file.mtime);

        let file_name = match name_override {
            Some(x) => {
                name_override = None;
                x
            }
            None => match CStr::from_bytes_until_nul(&current_file.name) {
                Ok(x) => x.to_bytes(),
                Err(_) => &current_file.name,
            },
        };

        match current_file.typ {
            REGULAR | NORMAL | CONTIGOUS => {
                let (dir, file_name) =
                    create_dirs(root.clone(), target.clone(), file_name, file_mode.clone())?;
                let file = File::open(
                    root.clone(),
                    dir,
                    file_name,
                    OpenFlags::Create,
                    file_mode,
                    Identity::get_kernel(),
                )?;
                file.pwrite(&data[offset + 512..][..file_size], 0)?;
                files_loaded += 1;

                let time = timespec {
                    tv_sec: (mtime / 1000000) as _,
                    tv_nsec: mtime as _,
                };
                file.inode
                    .as_ref()
                    .unwrap()
                    .update_time(Some(time), Some(time), Some(time));

                if BootInfo::get()
                    .command_line
                    .get_bool("module_autoload")
                    .unwrap_or(true)
                    && file_name.ends_with(b".kso")
                {
                    module::load(&data[offset + 512..][..file_size]).unwrap();
                }
            }
            SYM_LINK => {
                let (dir, file_name) =
                    create_dirs(root.clone(), target.clone(), file_name, file_mode)?;
                let link_len = current_file
                    .linkname
                    .iter()
                    .take_while(|&x| *x != 0)
                    .count();
                vfs::symlink(
                    root.clone(),
                    dir.clone(),
                    file_name,
                    &current_file.linkname[0..link_len],
                    Identity::get_kernel(),
                )?;
                files_loaded += 1;
            }
            DIRECTORY => {
                let (dir, file_name) =
                    create_dirs(root.clone(), target.clone(), file_name, file_mode.clone())?;
                create_dirs(root.clone(), dir.clone(), file_name, file_mode)?;
                files_loaded += 1;
            }
            LONG_LINK => {
                name_override = Some(&data[offset + 512..][..file_size - 1]); // -1 for the NUL terminator.
            }
            _ => (),
        }

        offset += 512 + util::align_up(file_size, 512);
    }

    log!("Loaded {files_loaded} files from initramfs");

    return Ok(());
}
