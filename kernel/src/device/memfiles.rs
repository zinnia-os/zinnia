use crate::{
    clock, device,
    memory::IovecIter,
    posix::errno::{EResult, Errno},
    process::PROCESS_STAGE,
    vfs::{File, file::FileOps, fs::devtmpfs::DEVTMPFS_STAGE, inode::Mode},
};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
pub struct NullFile;

impl FileOps for NullFile {
    fn read(&self, _: &File, _: &mut IovecIter, _: u64) -> EResult<isize> {
        Ok(0)
    }

    fn write(&self, _: &File, buffer: &mut IovecIter, _: u64) -> EResult<isize> {
        Ok(buffer.len() as _)
    }
}

#[derive(Debug)]
pub struct ZeroFile;

impl FileOps for ZeroFile {
    fn read(&self, _: &File, buffer: &mut IovecIter, _: u64) -> EResult<isize> {
        buffer.fill(0)?;
        Ok(buffer.len() as _)
    }

    fn write(&self, _: &File, buffer: &mut IovecIter, _: u64) -> EResult<isize> {
        Ok(buffer.len() as _)
    }
}

#[derive(Debug)]
pub struct FullFile;

impl FileOps for FullFile {
    fn read(&self, _: &File, buffer: &mut IovecIter, _: u64) -> EResult<isize> {
        buffer.fill(0)?;
        Ok(buffer.len() as _)
    }

    fn write(&self, _: &File, _: &mut IovecIter, _: u64) -> EResult<isize> {
        Err(Errno::ENOSPC)
    }
}

#[derive(Debug)]
pub struct RandomFile;

static RNG_STATE: AtomicU64 = AtomicU64::new(0x9e37_79b9_7f4a_7c15);

pub fn fill_random(buf: &mut [u8]) {
    let mut i = 0;
    while i < buf.len() {
        let bytes = next_random_u64().to_le_bytes();
        let take = (buf.len() - i).min(bytes.len());
        buf[i..i + take].copy_from_slice(&bytes[..take]);
        i += take;
    }
}

fn next_random_u64() -> u64 {
    let mut x = RNG_STATE.load(Ordering::Relaxed)
        ^ (clock::get_elapsed() as u64).wrapping_mul(0x2545_f491_4f6c_dd1d);
    if x == 0 {
        x = 0x9e37_79b9_7f4a_7c15;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    RNG_STATE.store(x, Ordering::Relaxed);
    x
}

impl FileOps for RandomFile {
    fn read(&self, _: &File, buffer: &mut IovecIter, _: u64) -> EResult<isize> {
        let total = buffer.len();
        let mut written = 0;
        let mut chunk = [0u8; 64];

        while written < total {
            let n = (total - written).min(chunk.len());
            let mut i = 0;
            while i < n {
                let bytes = next_random_u64().to_le_bytes();
                let take = (n - i).min(bytes.len());
                chunk[i..i + take].copy_from_slice(&bytes[..take]);
                i += take;
            }
            buffer.copy_from_slice(&chunk[..n])?;
            written += n;
        }

        Ok(total as isize)
    }

    fn write(&self, _: &File, buffer: &mut IovecIter, _: u64) -> EResult<isize> {
        // Accept and discard any entropy written back to the device.
        Ok(buffer.len() as isize)
    }
}

#[initgraph::task(
    name = "generic.device.memfiles",
    depends = [PROCESS_STAGE, DEVTMPFS_STAGE]
)]
fn MEMFILES_STAGE() {
    device::register_char_node(
        b"null",
        device::make_shared(Arc::new(NullFile), 1, 3),
        Mode::from_bits_truncate(0o666),
    )
    .expect("Unable to create /dev/null");

    device::register_char_node(
        b"full",
        device::make_shared(Arc::new(FullFile), 1, 7),
        Mode::from_bits_truncate(0o666),
    )
    .expect("Unable to create /dev/full");

    device::register_char_node(
        b"zero",
        device::make_shared(Arc::new(ZeroFile), 1, 5),
        Mode::from_bits_truncate(0o666),
    )
    .expect("Unable to create /dev/zero");

    device::register_char_node(
        b"random",
        device::make_shared(Arc::new(RandomFile), 1, 8),
        Mode::from_bits_truncate(0o666),
    )
    .expect("Unable to create /dev/random");

    device::register_char_node(
        b"urandom",
        device::make_shared(Arc::new(RandomFile), 1, 9),
        Mode::from_bits_truncate(0o666),
    )
    .expect("Unable to create /dev/urandom");
}
