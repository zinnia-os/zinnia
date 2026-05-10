use crate::{
    boot::BootInfo,
    device::net::{interface::ManagedInterface, l3::ipv4::Ipv4Addr, nic::NicDevice},
    memory::IovecIter,
    posix::errno::EResult,
    process::Identity,
    vfs::{
        self, File,
        file::FileOps,
        fs::devtmpfs,
        inode::{Device, Mode},
    },
};
use alloc::{format, sync::Arc};
use core::sync::atomic::{AtomicUsize, Ordering};

const MAX_FRAME_LEN: usize = 1518;
const DEFAULT_NETMASK: Ipv4Addr = Ipv4Addr::new([255, 255, 255, 0]);

static ETH_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct NicFile {
    nic: Arc<dyn NicDevice>,
}

impl FileOps for NicFile {
    fn read(&self, _file: &File, buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        let mut frame = [0u8; MAX_FRAME_LEN];
        let n = self.nic.recv(&mut frame)?;
        buffer.copy_from_slice(&frame[..n])
    }

    fn write(&self, _file: &File, buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        let mut frame = [0u8; MAX_FRAME_LEN];
        let n = buffer.len().min(MAX_FRAME_LEN);
        buffer.copy_to_slice(&mut frame[..n])?;
        self.nic.send(&frame[..n])?;
        Ok(n as isize)
    }
}

pub fn register_nic(nic: Arc<dyn NicDevice>) -> EResult<()> {
    if let Some(config) = configured_ipv4() {
        let interface = Arc::new(ManagedInterface::new(
            nic.clone(),
            nic.mac(),
            config.ip,
            config.netmask,
            config.gateway,
        ));
        if let Some(gateway) = config.gateway {
            log!(
                "Bringing up managed interface {} ({}) netmask={} gateway={}",
                config.ip,
                nic.mac(),
                config.netmask,
                gateway
            );
        } else {
            log!(
                "Bringing up managed interface {} ({}) netmask={} gateway=none",
                config.ip,
                nic.mac(),
                config.netmask
            );
        }
        super::interface::start_worker(interface.clone())?;
        super::interface::register_interface(interface);
        return Ok(());
    }

    let idx = ETH_COUNTER.fetch_add(1, Ordering::SeqCst);
    let name = format!("net/eth{}", idx);

    let root = devtmpfs::get_root();
    vfs::mknod(
        root.clone(),
        root,
        name.as_bytes(),
        Mode::from_bits_truncate(0o660),
        Some(Device::CharacterDevice(Arc::new(NicFile { nic }))),
        &Identity::get_kernel(),
    )
}

struct Ipv4Config {
    ip: Ipv4Addr,
    netmask: Ipv4Addr,
    gateway: Option<Ipv4Addr>,
}

fn configured_ipv4() -> Option<Ipv4Config> {
    let raw = BootInfo::get().command_line.get_string("ip")?;
    let Some(ip) = parse_ipv4_cmdline("ip", raw) else {
        return None;
    };

    Some(Ipv4Config {
        ip,
        netmask: configured_netmask(),
        gateway: configured_gateway(),
    })
}

fn configured_netmask() -> Ipv4Addr {
    let Some(raw) = BootInfo::get().command_line.get_string("netmask") else {
        return DEFAULT_NETMASK;
    };

    parse_ipv4_cmdline("netmask", raw).unwrap_or(DEFAULT_NETMASK)
}

fn configured_gateway() -> Option<Ipv4Addr> {
    let raw = BootInfo::get()
        .command_line
        .get_string("gateway")
        .or_else(|| BootInfo::get().command_line.get_string("gw"))?;

    parse_ipv4_cmdline("gateway", raw)
}

fn parse_ipv4_cmdline(name: &str, raw: &str) -> Option<Ipv4Addr> {
    match Ipv4Addr::parse(raw) {
        Some(addr) => Some(addr),
        None => {
            log!("{}=\"{}\" is not a valid IPv4 address; ignoring", name, raw);
            None
        }
    }
}

#[initgraph::task(
    name = "generic.device.net",
    depends = [devtmpfs::DEVTMPFS_STAGE],
)]
pub fn NET_DEVICE_STAGE() {
    let root = devtmpfs::get_root();
    vfs::mkdir(
        root.clone(),
        root,
        b"net",
        Mode::from_bits_truncate(0o755),
        &Identity::get_kernel(),
    )
    .expect("Unable to create /dev/net");
}
