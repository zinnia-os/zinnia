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
    util::mutex::spin::SpinMutex,
};
use alloc::{sync::Arc, vec::Vec};

pub const RX_FRAME_LEN: usize = 1518;
pub const MAX_ETH_PAYLOAD_LEN: usize = 1500;
pub const MAX_IPV4_PAYLOAD_LEN: usize = MAX_ETH_PAYLOAD_LEN - IPV4_HEADER_LEN;

static INTERFACES: SpinMutex<Vec<Arc<ManagedInterface>>> = SpinMutex::new(Vec::new());

pub struct ManagedInterface {
    nic: Arc<dyn NicDevice>,
    mac: MacAddr,
    ip: Ipv4Addr,
    netmask: Ipv4Addr,
    gateway: Option<Ipv4Addr>,
    arp_cache: ArpCache,
}

impl ManagedInterface {
    pub fn new(
        nic: Arc<dyn NicDevice>,
        mac: MacAddr,
        ip: Ipv4Addr,
        netmask: Ipv4Addr,
        gateway: Option<Ipv4Addr>,
    ) -> Self {
        Self {
            nic,
            mac,
            ip,
            netmask,
            gateway,
            arp_cache: ArpCache::new(),
        }
    }

    pub fn mac(&self) -> MacAddr {
        self.mac
    }

    pub fn ip(&self) -> Ipv4Addr {
        self.ip
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

        self.gateway.unwrap_or(dst)
    }

    fn is_local_ipv4(&self, dst: Ipv4Addr) -> bool {
        let mask = self.netmask.as_u32();
        self.ip.as_u32() & mask == dst.as_u32() & mask
    }
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
