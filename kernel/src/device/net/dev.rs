use crate::{
    device::net::{interface::ManagedInterface, l3::ipv4::Ipv4Addr, nic::NicDevice},
    posix::errno::EResult,
    process::Identity,
    uapi::net::IFNAMSIZ,
    vfs::{self, fs::devtmpfs, inode::Mode},
};
use alloc::{format, sync::Arc};
use core::sync::atomic::{AtomicU32, Ordering};

const DEFAULT_NETMASK: Ipv4Addr = Ipv4Addr::new([255, 255, 255, 0]);

static ETH_COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn register_nic(nic: Arc<dyn NicDevice>) -> EResult<()> {
    let idx = ETH_COUNTER.fetch_add(1, Ordering::SeqCst);
    let (ip, netmask, gateway) = (Ipv4Addr::ANY, DEFAULT_NETMASK, None);

    let interface = Arc::new(ManagedInterface::new(
        nic.clone(),
        nic.mac(),
        {
            let mut name = [0u8; IFNAMSIZ];
            let s = format!("eth{idx}");
            let n = s.len().min(IFNAMSIZ - 1);
            name[..n].copy_from_slice(&s.as_bytes()[..n]);
            name
        },
        idx + 1,
        ip,
        netmask,
        gateway,
    ));

    if ip == Ipv4Addr::ANY {
        log!("Registered interface eth{} ({})", idx, nic.mac());
    } else if let Some(gateway) = gateway {
        log!(
            "Bringing up eth{} {} ({}) netmask={} gateway={}",
            idx,
            ip,
            nic.mac(),
            netmask,
            gateway
        );
    } else {
        log!(
            "Bringing up eth{} {} ({}) netmask={} gateway=none",
            idx,
            ip,
            nic.mac(),
            netmask
        );
    }

    super::interface::start_worker(interface.clone())?;
    super::interface::register_interface(interface);
    Ok(())
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
