pub const IOC_VOID: u32 = 0;
pub const IOC_OUT: u32 = 0x40000000;
pub const IOC_IN: u32 = 0x80000000;
pub const IOC_INOUT: u32 = IOC_IN | IOC_OUT;
pub const IOCPARM_SHIFT: u32 = 13;
pub const IOCPARM_MASK: u32 = (1 << IOCPARM_SHIFT) - 1;

pub const fn ioc(inout: u32, group: u8, num: u8, len: u16) -> u32 {
    inout | ((len as u32 & IOCPARM_MASK) << 16) | ((group as u32) << 8) | num as u32
}

pub const fn ioc_nr(cmd: u32) -> u8 {
    (cmd & 0xFF) as u8
}

pub const fn ioc_size(cmd: u32) -> u32 {
    (cmd >> 16) & 0x3FFF
}

pub const fn ioc_type(cmd: u32) -> u8 {
    ((cmd >> 8) & 0xFF) as u8
}

pub const fn io(group: u8, num: u8) -> u32 {
    ioc(IOC_VOID, group, num, 0)
}

pub const fn iowint(group: u8, num: u8) -> u32 {
    ioc(IOC_VOID, group, num, size_of::<i32>() as _)
}

pub const fn ior<T>(group: u8, num: u8) -> u32 {
    ioc(IOC_OUT, group, num, size_of::<T>() as _)
}

pub const fn iow<T>(group: u8, num: u8) -> u32 {
    ioc(IOC_IN, group, num, size_of::<T>() as _)
}

pub const fn iowr<T>(group: u8, num: u8) -> u32 {
    ioc(IOC_INOUT, group, num, size_of::<T>() as _)
}

pub const RTC_RD_TIME: u32 = 1;
pub const RTC_SET_TIME: u32 = 2;
pub const FIOQSIZE: u32 = io(b'T', 0x60);
pub const TCGETS: u32 = io(b'T', 0x01);
pub const TCSETS: u32 = io(b'T', 0x02);
pub const TCSETSW: u32 = io(b'T', 0x03);
pub const TCSETSF: u32 = io(b'T', 0x04);
pub const TCGETA: u32 = io(b'T', 0x05);
pub const TCSETA: u32 = io(b'T', 0x06);
pub const TCSETAW: u32 = io(b'T', 0x07);
pub const TCSETAF: u32 = io(b'T', 0x08);
pub const TCSBRK: u32 = io(b'T', 0x09);
pub const TCXONC: u32 = io(b'T', 0x0A);
pub const TCFLSH: u32 = io(b'T', 0x0B);
pub const TIOCEXCL: u32 = io(b'T', 0x0C);
pub const TIOCNXCL: u32 = io(b'T', 0x0D);
pub const TIOCSCTTY: u32 = io(b'T', 0x0E);
pub const TIOCGPGRP: u32 = io(b'T', 0x0F);
pub const TIOCSPGRP: u32 = io(b'T', 0x10);
pub const TIOCOUTQ: u32 = io(b'T', 0x11);
pub const TIOCSTI: u32 = io(b'T', 0x12);
pub const TIOCGWINSZ: u32 = io(b'T', 0x13);
pub const TIOCSWINSZ: u32 = io(b'T', 0x14);
pub const TIOCMGET: u32 = io(b'T', 0x15);
pub const TIOCMBIS: u32 = io(b'T', 0x16);
pub const TIOCMBIC: u32 = io(b'T', 0x17);
pub const TIOCMSET: u32 = io(b'T', 0x18);
pub const TIOCGSOFTCAR: u32 = io(b'T', 0x19);
pub const TIOCSSOFTCAR: u32 = io(b'T', 0x1A);
pub const FIONREAD: u32 = io(b'T', 0x1b);
pub const TIOCINQ: u32 = FIONREAD;
pub const TIOCLINUX: u32 = io(b'T', 0x1C);
pub const TIOCCONS: u32 = io(b'T', 0x1D);
pub const TIOCGSERIAL: u32 = io(b'T', 0x1E);
pub const TIOCSSERIAL: u32 = io(b'T', 0x1F);
pub const TIOCPKT: u32 = io(b'T', 0x20);
pub const FIONBIO: u32 = io(b'T', 0x21);
pub const TIOCNOTTY: u32 = io(b'T', 0x22);
pub const TIOCSETD: u32 = io(b'T', 0x23);
pub const TIOCGETD: u32 = io(b'T', 0x24);
pub const TCSBRKP: u32 = io(b'T', 0x25);
pub const TIOCSBRK: u32 = io(b'T', 0x27);
pub const TIOCCBRK: u32 = io(b'T', 0x28);
pub const TIOCGSID: u32 = io(b'T', 0x29);
pub const TIOCGNAME: u32 = io(b'T', 0x70);
pub const TCGETS2: u32 = 3;
pub const TCSETS2: u32 = 3;
pub const TCSETSW2: u32 = 3;
pub const TCSETSF2: u32 = 3;
pub const TIOCGRS485: u32 = io(b'T', 0x2E);
pub const TIOCSRS485: u32 = io(b'T', 0x2F);
pub const TIOCGPTN: u32 = ior::<u32>(b'T', 0x30);
pub const TIOCSPTLCK: u32 = iow::<i32>(b'T', 0x31);
pub const TIOCGDEV: u32 = 3;
pub const TCGETX: u32 = io(b'T', 0x32);
pub const TCSETX: u32 = io(b'T', 0x33);
pub const TCSETXF: u32 = io(b'T', 0x34);
pub const TCSETXW: u32 = io(b'T', 0x35);
pub const TIOCSIG: u32 = 0x36;
pub const TIOCVHANGUP: u32 = io(b'T', 0x37);
pub const TIOCGPKT: u32 = 3;
pub const TIOCGPTLCK: u32 = 3;
pub const TIOCGEXCL: u32 = 3;
pub const TIOCGPTPEER: u32 = 3;
pub const TIOCGISO7816: u32 = 3;
pub const TIOCSISO7816: u32 = 3;
pub const FIONCLEX: u32 = io(b'T', 0x50);
pub const FIOCLEX: u32 = io(b'T', 0x51);
pub const FIOASYNC: u32 = io(b'T', 0x52);
pub const TIOCSERCONFIG: u32 = io(b'T', 0x53);
pub const TIOCSERGWILD: u32 = io(b'T', 0x54);
pub const TIOCSERSWILD: u32 = io(b'T', 0x55);
pub const TIOCGLCKTRMIOS: u32 = io(b'T', 0x56);
pub const TIOCSLCKTRMIOS: u32 = io(b'T', 0x57);
pub const TIOCSERGSTRUCT: u32 = io(b'T', 0x58);
pub const TIOCSERGETLSR: u32 = io(b'T', 0x59);
pub const TIOCSERGETMULTI: u32 = io(b'T', 0x5A);
pub const TIOCSERSETMULTI: u32 = io(b'T', 0x5B);
pub const TIOCMIWAIT: u32 = io(b'T', 0x5C);
pub const TIOCGICOUNT: u32 = io(b'T', 0x5D);
pub const TIOCPKT_DATA: u32 = 0;
pub const TIOCPKT_FLUSHREAD: u32 = 1;
pub const TIOCPKT_FLUSHWRITE: u32 = 2;
pub const TIOCPKT_STOP: u32 = 4;
pub const TIOCPKT_START: u32 = 8;
pub const TIOCPKT_NOSTOP: u32 = 16;
pub const TIOCPKT_DOSTOP: u32 = 32;
pub const TIOCPKT_IOCTL: u32 = 64;
pub const TIOCSER_TEMT: u32 = 0x01;
