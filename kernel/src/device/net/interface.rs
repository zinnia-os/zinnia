use crate::{
    device::net::{
        l2::{
            arp::{self, ArpCache},
            eth::{ETH_HEADER_LEN, EthHeader, EtherType, write_header},
            mac::MacAddr,
        },
        l3::ipv4::{IPV4_HEADER_LEN, Ipv4Addr},
        nic::NicDevice,
    },
    posix::errno::{EResult, Errno},
    process::{Process, task::Task},
    sched::Scheduler,
    uapi::net::{IFF_BROADCAST, IFF_MULTICAST, IFF_RUNNING, IFF_UP, IFNAMSIZ},
    util::mutex::spin::SpinMutex,
};
use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU16, AtomicU32, Ordering};

pub const RX_FRAME_LEN: usize = 1518;
pub const MAX_ETH_PAYLOAD_LEN: usize = 1500;
pub const MAX_IPV4_PAYLOAD_LEN: usize = MAX_ETH_PAYLOAD_LEN - IPV4_HEADER_LEN;

static INTERFACES: SpinMutex<Vec<Arc<ManagedInterface>>> = SpinMutex::new(Vec::new());

pub struct ManagedInterface {
    nic: Arc<dyn NicDevice>,
    mac: MacAddr,
    name: [u8; IFNAMSIZ],
    index: u32,
    ip: AtomicU32,
    netmask: AtomicU32,
    gateway: AtomicU32,
    flags: AtomicU16,
    arp_cache: ArpCache,
}

impl ManagedInterface {
    pub fn new(
        nic: Arc<dyn NicDevice>,
        mac: MacAddr,
        name: [u8; IFNAMSIZ],
        index: u32,
        ip: Ipv4Addr,
        netmask: Ipv4Addr,
        gateway: Option<Ipv4Addr>,
    ) -> Self {
        let flags = IFF_UP | IFF_RUNNING | IFF_BROADCAST | IFF_MULTICAST;
        Self {
            nic,
            mac,
            name,
            index,
            ip: AtomicU32::new(ip.as_u32()),
            netmask: AtomicU32::new(netmask.as_u32()),
            gateway: AtomicU32::new(gateway.map_or(0, Ipv4Addr::as_u32)),
            flags: AtomicU16::new(flags as u16),
            arp_cache: ArpCache::new(),
        }
    }

    pub fn mac(&self) -> MacAddr {
        self.mac
    }

    pub fn name(&self) -> &[u8; IFNAMSIZ] {
        &self.name
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn ip(&self) -> Ipv4Addr {
        Ipv4Addr::from_u32(self.ip.load(Ordering::Acquire))
    }

    pub fn set_ip(&self, ip: Ipv4Addr) {
        self.ip.store(ip.as_u32(), Ordering::Release);
    }

    pub fn netmask(&self) -> Ipv4Addr {
        Ipv4Addr::from_u32(self.netmask.load(Ordering::Acquire))
    }

    pub fn set_netmask(&self, netmask: Ipv4Addr) {
        self.netmask.store(netmask.as_u32(), Ordering::Release);
    }

    pub fn gateway(&self) -> Option<Ipv4Addr> {
        match self.gateway.load(Ordering::Acquire) {
            0 => None,
            v => Some(Ipv4Addr::from_u32(v)),
        }
    }

    pub fn set_gateway(&self, gateway: Option<Ipv4Addr>) {
        self.gateway
            .store(gateway.map_or(0, Ipv4Addr::as_u32), Ordering::Release);
    }

    pub fn flags(&self) -> i16 {
        self.flags.load(Ordering::Acquire) as i16
    }

    pub fn set_flags(&self, flags: i16) {
        self.flags.store(flags as u16, Ordering::Release);
    }

    pub fn arp_cache(&self) -> &ArpCache {
        &self.arp_cache
    }

    pub fn process_frame(&self, frame: &[u8]) -> EResult<bool> {
        let Some(eth) = EthHeader::parse(frame) else {
            return Ok(false);
        };

        match eth.ethertype() {
            Some(EtherType::Arp) => crate::device::net::l2::arp::process_packet(self, &eth),
            Some(EtherType::Ipv4) => crate::device::net::l3::ipv4::process_packet(self, &eth),
            _ => Ok(false),
        }
    }

    pub fn send_ethernet(&self, dst: MacAddr, ethertype: EtherType, payload: &[u8]) -> EResult<()> {
        if payload.len() > MAX_ETH_PAYLOAD_LEN {
            return Err(Errno::EMSGSIZE);
        }

        let mut frame = [0u8; RX_FRAME_LEN];
        let body = write_header(&mut frame, &dst, &self.mac, ethertype).ok_or(Errno::EINVAL)?;
        body[..payload.len()].copy_from_slice(payload);
        self.nic.send(&frame[..ETH_HEADER_LEN + payload.len()])
    }

    pub fn send_raw(&self, frame: &[u8]) -> EResult<()> {
        if frame.len() < ETH_HEADER_LEN || frame.len() > RX_FRAME_LEN {
            return Err(Errno::EMSGSIZE);
        }
        self.nic.send(frame)
    }

    pub fn resolve_ipv4(&self, dst: Ipv4Addr) -> EResult<MacAddr> {
        let next_hop = self.ipv4_next_hop(dst);
        if next_hop == Ipv4Addr::BROADCAST {
            return Ok(MacAddr::BROADCAST);
        }

        if let Some(mac) = self.arp_cache.lookup(&next_hop) {
            return Ok(mac);
        }

        arp::send_request(self, next_hop)?;
        Err(Errno::EHOSTUNREACH)
    }

    pub fn ipv4_next_hop(&self, dst: Ipv4Addr) -> Ipv4Addr {
        if dst == Ipv4Addr::BROADCAST || self.is_local_ipv4(dst) {
            return dst;
        }

        self.gateway().unwrap_or(dst)
    }

    fn is_local_ipv4(&self, dst: Ipv4Addr) -> bool {
        let mask = self.netmask().as_u32();
        self.ip().as_u32() & mask == dst.as_u32() & mask
    }

    /// Directed broadcast address for this interface's subnet.
    pub fn broadcast_ipv4(&self) -> Ipv4Addr {
        Ipv4Addr::from_u32(self.ip().as_u32() | !self.netmask().as_u32())
    }
}

fn name_bytes(buf: &[u8]) -> &[u8] {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    &buf[..end]
}

pub fn by_name(name: &[u8]) -> Option<Arc<ManagedInterface>> {
    let name = name_bytes(name);
    INTERFACES
        .lock()
        .iter()
        .find(|iface| name_bytes(iface.name()) == name)
        .cloned()
}

pub fn by_index(index: u32) -> Option<Arc<ManagedInterface>> {
    INTERFACES
        .lock()
        .iter()
        .find(|iface| iface.index() == index)
        .cloned()
}

pub fn snapshot() -> Vec<Arc<ManagedInterface>> {
    INTERFACES.lock().clone()
}

pub fn register_interface(interface: Arc<ManagedInterface>) {
    INTERFACES.lock().push(interface);
}

pub fn default_ipv4_interface() -> Option<Arc<ManagedInterface>> {
    INTERFACES.lock().first().cloned()
}

pub fn interface_for_source(source: Ipv4Addr) -> Option<Arc<ManagedInterface>> {
    let interfaces = INTERFACES.lock();
    interfaces
        .iter()
        .find(|interface| source == Ipv4Addr::ANY || interface.ip() == source)
        .cloned()
}

/// Spawn a kernel mode worker that owns frame reception on this interface's NIC.
pub fn start_worker(interface: Arc<ManagedInterface>) -> EResult<()> {
    let raw = Arc::into_raw(interface) as usize;

    let task = Task::new(rx_worker_entry, raw, 0, Process::get_kernel(), false);
    match task {
        Ok(t) => {
            Scheduler::add_task_to_best_cpu(Arc::new(t));
            Ok(())
        }
        Err(e) => {
            unsafe {
                let _ = Arc::from_raw(raw as *const ManagedInterface);
            }
            Err(e)
        }
    }
}

extern "C" fn rx_worker_entry(arg1: usize, _arg2: usize) {
    let interface = unsafe { Arc::from_raw(arg1 as *const ManagedInterface) };
    let mut frame = [0u8; RX_FRAME_LEN];

    log!("Worker started on {} ({})", interface.ip(), interface.mac());

    loop {
        match interface.nic.recv(&mut frame) {
            Ok(n) => {
                crate::device::net::l2::packet::deliver(interface.index(), &frame[..n]);
                if let Err(e) = interface.process_frame(&frame[..n]) {
                    log!("process_frame failed: {:?}", e);
                }
            }
            Err(e) => {
                log!("nic.recv failed: {:?}", e);
            }
        }
    }
}
