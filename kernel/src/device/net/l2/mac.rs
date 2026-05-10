use core::fmt::{Display, Formatter};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacAddr {
    buf: [u8; 6],
}

impl MacAddr {
    pub const BROADCAST: MacAddr = MacAddr::new(&[0xff; 6]);
    pub const ZERO: MacAddr = MacAddr::new(&[0; 6]);

    pub const fn new(bytes: &[u8; 6]) -> Self {
        let mut buf = [0u8; 6];
        let mut i = 0;
        while i < 6 {
            buf[i] = bytes[i];
            i += 1;
        }
        Self { buf }
    }

    pub const fn as_bytes(&self) -> &[u8; 6] {
        &self.buf
    }
}

impl Display for MacAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.buf[0], self.buf[1], self.buf[2], self.buf[3], self.buf[4], self.buf[5]
        ))
    }
}
