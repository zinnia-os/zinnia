type cc_t = u8;
type speed_t = u32;
type tcflag_t = u32;

pub const NCCS: usize = 32;
pub const VINTR: u32 = 0;
pub const VQUIT: u32 = 1;
pub const VERASE: u32 = 2;
pub const VKILL: u32 = 3;
pub const VEOF: u32 = 4;
pub const VTIME: u32 = 5;
pub const VMIN: u32 = 6;
pub const VSWTC: u32 = 7;
pub const VSTART: u32 = 8;
pub const VSTOP: u32 = 9;
pub const VSUSP: u32 = 10;
pub const VEOL: u32 = 11;
pub const VREPRINT: u32 = 12;
pub const VDISCARD: u32 = 13;
pub const VWERASE: u32 = 14;
pub const VLNEXT: u32 = 15;
pub const VEOL2: u32 = 16;

pub const IGNBRK: u32 = 0o000001;
pub const BRKINT: u32 = 0o000002;
pub const IGNPAR: u32 = 0o000004;
pub const PARMRK: u32 = 0o000010;
pub const INPCK: u32 = 0o000020;
pub const ISTRIP: u32 = 0o000040;
pub const INLCR: u32 = 0o000100;
pub const IGNCR: u32 = 0o000200;
pub const ICRNL: u32 = 0o000400;
pub const IUCLC: u32 = 0o001000;
pub const IXON: u32 = 0o002000;
pub const IXANY: u32 = 0o004000;
pub const IXOFF: u32 = 0o010000;
pub const IMAXBEL: u32 = 0o020000;
pub const IUTF8: u32 = 0o040000;

pub const OPOST: u32 = 0o000001;
pub const OLCUC: u32 = 0o000002;
pub const ONLCR: u32 = 0o000004;
pub const OCRNL: u32 = 0o000010;
pub const ONOCR: u32 = 0o000020;
pub const ONLRET: u32 = 0o000040;
pub const OFILL: u32 = 0o000100;
pub const OFDEL: u32 = 0o000200;

pub const NLDLY: u32 = 0o000400;
pub const NL0: u32 = 0o000000;
pub const NL1: u32 = 0o000400;

pub const CRDLY: u32 = 0o003000;
pub const CR0: u32 = 0o000000;
pub const CR1: u32 = 0o001000;
pub const CR2: u32 = 0o002000;
pub const CR3: u32 = 0o003000;

pub const TABDLY: u32 = 0o014000;
pub const TAB0: u32 = 0o000000;
pub const TAB1: u32 = 0o004000;
pub const TAB2: u32 = 0o010000;
pub const TAB3: u32 = 0o014000;

pub const BSDLY: u32 = 0o020000;
pub const BS0: u32 = 0o000000;
pub const BS1: u32 = 0o020000;

pub const FFDLY: u32 = 0o100000;
pub const FF0: u32 = 0o000000;
pub const FF1: u32 = 0o100000;

pub const VTDLY: u32 = 0o040000;
pub const VT0: u32 = 0o000000;
pub const VT1: u32 = 0o040000;

pub const CSIZE: u32 = 0o000060;
pub const CS5: u32 = 0o000000;
pub const CS6: u32 = 0o000020;
pub const CS7: u32 = 0o000040;
pub const CS8: u32 = 0o000060;

pub const CSTOPB: u32 = 0o000100;
pub const CREAD: u32 = 0o000200;
pub const PARENB: u32 = 0o000400;
pub const PARODD: u32 = 0o001000;
pub const HUPCL: u32 = 0o002000;
pub const CLOCAL: u32 = 0o004000;

pub const ISIG: u32 = 0o000001;
pub const ICANON: u32 = 0o000002;
pub const ECHO: u32 = 0o000010;
pub const ECHOE: u32 = 0o000020;
pub const ECHOK: u32 = 0o000040;
pub const ECHONL: u32 = 0o000100;
pub const NOFLSH: u32 = 0o000200;
pub const TOSTOP: u32 = 0o000400;
pub const IEXTEN: u32 = 0o100000;

pub const TCIFLUSH: usize = 0;
pub const TCOFLUSH: usize = 1;
pub const TCIOFLUSH: usize = 2;

pub const EXTA: u32 = 0o000016;
pub const EXTB: u32 = 0o000017;
pub const CBAUD: u32 = 0o010017;
pub const CBAUDEX: u32 = 0o010000;
pub const CIBAUD: u32 = 0o02003600000;
pub const CMSPAR: u32 = 0o10000000000;
pub const CRTSCTS: u32 = 0o20000000000;

pub const XCASE: u32 = 0o000004;
pub const ECHOCTL: u32 = 0o001000;
pub const ECHOPRT: u32 = 0o002000;
pub const ECHOKE: u32 = 0o004000;
pub const FLUSHO: u32 = 0o010000;
pub const PENDIN: u32 = 0o040000;
pub const EXTPROC: u32 = 0o200000;

pub const XTABS: u32 = 0o014000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct termios {
    pub c_iflag: tcflag_t,
    pub c_oflag: tcflag_t,
    pub c_cflag: tcflag_t,
    pub c_lflag: tcflag_t,
    pub c_line: cc_t,
    pub c_cc: [cc_t; NCCS],
    pub ibaud: speed_t,
    pub obaud: speed_t,
}

pub const NCC: usize = 8;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct termio {
    pub c_iflag: u16,
    pub c_oflag: u16,
    pub c_cflag: u16,
    pub c_lflag: u16,
    pub c_line: u8,
    pub c_cc: [u8; NCC],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}
