zinnia::bitflags::bitflags! {
    pub struct FeatureFlags: u64 {
        const Csum = 1 << 0;
        const GuestCsum = 1 << 1;
        const CtrlGuestOffloads = 1 << 2;
        const Mtu = 1 << 3;
        const Mac = 1 << 5;
        const GuestTSO4 = 1 << 7;
        const GuestTSO6 = 1 << 8;
        const GuestEcn = 1 << 9;
        const GuestUfo = 1 << 10;
        const HostTSO4 = 1 << 11;
        const HostTSO6 = 1 << 12;
        const HostEcn = 1 << 13;
        const HostUfo = 1 << 14;
        const MrgRxbuf = 1 << 15;
        const Status = 1 << 16;
        const CtrlVq = 1 << 17;
        const CtrlRx = 1 << 18;
        const CtrlVlan = 1 << 19;
        const CtrlRxExtra = 1 << 20;
        const GuestAnnounce = 1 << 21;
        const Mq = 1 << 22;
        const CtrlMacAddr = 1 << 23;
        const HashTunnel = 1 << 51;
        const VqNotfCoal = 1 << 52;
        const NotfCoal = 1 << 53;
        const GuestUSO4 = 1 << 54;
        const GuestUSO6 = 1 << 55;
        const HostUso = 1 << 56;
        const HashReport = 1 << 57;
        const GuestHdrlen = 1 << 59;
        const Rss = 1 << 60;
        const RscExt = 1 << 61;
        const Standby = 1 << 62;
        const SpeedDuplex = 1 << 63;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VirtHeader {
    pub flags: u8,
    pub gso_type: u8,
    pub hdr_len: u16,
    pub gso_size: u16,
    pub csum_start: u16,
    pub csum_offset: u16,
    pub num_buffers: u16,
}
