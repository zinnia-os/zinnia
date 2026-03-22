use crate::{uapi::utsname::utsname, util::mutex::spin::SpinMutex};

const SYSNAME: &[u8] = b"Zinnia";
const NODENAME: &[u8] = b"localhost";
const RELEASE: &[u8] = env!("CARGO_PKG_VERSION").as_bytes();
const VERSION: &[u8] = b"DEFAULT";
const DOMAINNAME: &[u8] = b"(none)";

#[cfg(target_arch = "x86_64")]
const MACHINE: &[u8] = b"x86_64";
#[cfg(target_arch = "aarch64")]
const MACHINE: &[u8] = b"aarch64";
#[cfg(target_arch = "riscv64")]
const MACHINE: &[u8] = b"riscv64";
#[cfg(target_arch = "loongarch64")]
const MACHINE: &[u8] = b"loongarch64";

pub static UTSNAME: SpinMutex<utsname> = SpinMutex::new({
    let mut name = utsname {
        sysname: [0; 65],
        nodename: [0; 65],
        release: [0; 65],
        version: [0; 65],
        machine: [0; 65],
        domainname: [0; 65],
    };

    name.sysname[0..SYSNAME.len()].copy_from_slice(SYSNAME);
    name.nodename[0..NODENAME.len()].copy_from_slice(NODENAME);
    name.release[0..RELEASE.len()].copy_from_slice(RELEASE);
    name.version[0..VERSION.len()].copy_from_slice(VERSION);
    name.domainname[0..DOMAINNAME.len()].copy_from_slice(DOMAINNAME);
    name.machine[0..MACHINE.len()].copy_from_slice(MACHINE);

    name
});
