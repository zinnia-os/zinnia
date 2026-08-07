#[derive(Clone, Copy)]
struct Channel {
    size: u8,
    shift: u8,
}

#[derive(Clone, Copy)]
pub(super) struct PixelFormat {
    red: Channel,
    green: Channel,
    blue: Channel,
}

impl PixelFormat {
    pub(super) fn new(
        red_size: u8,
        red_shift: u8,
        green_size: u8,
        green_shift: u8,
        blue_size: u8,
        blue_shift: u8,
    ) -> Self {
        let ch = |size: u8, shift: u8| Channel {
            size,
            shift: shift + (size - 8),
        };
        Self {
            red: ch(red_size, red_shift),
            green: ch(green_size, green_shift),
            blue: ch(blue_size, blue_shift),
        }
    }

    /// Convert an `0x00RRGGBB` colour into a device pixel.
    pub(super) fn convert(&self, colour: u32) -> u32 {
        let r = (colour >> 16) & 0xff;
        let g = (colour >> 8) & 0xff;
        let b = colour & 0xff;
        let mut ret = (r << self.red.shift) | (g << self.green.shift) | (b << self.blue.shift);
        for (chan, v) in [(self.red, r), (self.green, g), (self.blue, b)] {
            if chan.size > 8 {
                ret |= (v >> (16 - chan.size)) << (chan.shift as u32 - chan.size as u32 + 8);
            }
        }
        ret
    }
}

#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub(super) ansi: [u32; 8],
    pub(super) bright: [u32; 8],
    pub(super) default_fg: u32,
    pub(super) default_bg: u32,
    pub(super) default_fg_bright: u32,
    pub(super) default_bg_bright: u32,
}

impl Palette {
    pub(super) fn new(fmt: &PixelFormat) -> Self {
        let c = |rgb| fmt.convert(rgb);
        Self {
            ansi: [
                c(0x000000),
                c(0xaa0000),
                c(0x00aa00),
                c(0xaa5500),
                c(0x0000aa),
                c(0xaa00aa),
                c(0x00aaaa),
                c(0xaaaaaa),
            ],
            bright: [
                c(0x555555),
                c(0xff5555),
                c(0x55ff55),
                c(0xffff55),
                c(0x5555ff),
                c(0xff55ff),
                c(0x55ffff),
                c(0xffffff),
            ],
            default_fg: c(0xaaaaaa),
            default_bg: 0x0000_0000,
            default_fg_bright: c(0xffffff),
            default_bg_bright: c(0x555555),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Ink {
    Default,
    Indexed(u8),
    Rgb,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CellColor {
    Default,
    Explicit(u32),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Cell {
    pub(super) glyph: u8,
    pub(super) fg: CellColor,
    pub(super) bg: CellColor,
}

#[derive(Clone, Copy)]
pub(super) struct Pen {
    pub(super) fg: CellColor,
    pub(super) bg: CellColor,
    pub(super) primary: Ink,
    pub(super) bg_ink: Ink,
    pub(super) bold: bool,
    pub(super) bg_bold: bool,
    pub(super) reverse: bool,
}

impl Pen {
    pub(super) fn new(palette: &Palette) -> Self {
        Self {
            fg: CellColor::Explicit(palette.default_fg),
            bg: CellColor::Default,
            primary: Ink::Default,
            bg_ink: Ink::Default,
            bold: false,
            bg_bold: false,
            reverse: false,
        }
    }
}
