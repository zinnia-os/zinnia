const MAX_PARAMS: usize = 16;

pub(super) trait Perform {
    fn print_byte(&mut self, c: u8);
    fn print_unicode(&mut self, cp: u32);
    fn print_error(&mut self, update_last: bool);
    fn execute(&mut self, c: u8, in_csi: bool);
    fn csi_dispatch(&mut self, final_byte: u8, params: &Params, private: bool);
    fn esc_dispatch(&mut self, final_byte: u8);
    fn designate_charset(&mut self, g: u8, final_byte: u8);
    fn cancel(&mut self);
    fn ris(&mut self);
}

#[derive(Clone, Copy, Default)]
pub(super) struct Params {
    vals: [u32; MAX_PARAMS],
    len: usize,
}

impl Params {
    fn clear(&mut self) {
        self.vals.fill(0);
        self.len = 0;
    }

    fn is_full(&self) -> bool {
        self.len >= MAX_PARAMS
    }

    fn accumulate(&mut self, c: u8) {
        let slot = &mut self.vals[self.len];
        *slot = slot.saturating_mul(10).saturating_add((c - b'0') as u32);
    }

    fn commit(&mut self) {
        if !self.is_full() {
            self.len += 1;
        }
    }

    fn push_empty(&mut self) {
        if self.len < MAX_PARAMS {
            self.vals[self.len] = 0;
            self.len += 1;
        }
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn get(&self, i: usize, default: u32) -> u32 {
        if i >= self.len {
            return default;
        }
        let v = self.vals[i];
        if default != 0 && v == 0 { default } else { v }
    }
}

#[derive(Clone, Copy, Default)]
struct Utf8 {
    cp: u64,
    remaining: u8,
}

impl Utf8 {
    fn reset(&mut self) {
        self.remaining = 0;
        self.cp = 0;
    }

    fn start(&mut self, lead: u8) {
        if (0xc2..=0xdf).contains(&lead) {
            self.remaining = 1;
            self.cp = ((lead & 0x1f) as u64) << 6;
        } else if (0xe0..=0xef).contains(&lead) {
            self.remaining = 2;
            self.cp = ((lead & 0x0f) as u64) << 12;
        } else {
            self.remaining = 3;
            self.cp = ((lead & 0x07) as u64) << 18;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum State {
    #[default]
    Ground,
    CharsetSelect {
        g: u8,
    },
    Escape,
    CsiEntry,
    CsiParam,
    CsiIgnore,
    Osc,
    OscEsc,
}

impl State {
    fn is_escape(self) -> bool {
        !matches!(self, State::Ground | State::CharsetSelect { .. })
    }
}

#[derive(Default)]
pub(super) struct Parser {
    state: State,
    params: Params,
    partial: bool,
    private: bool,
    discard: bool,
    utf8: Utf8,
}

impl Parser {
    pub(super) fn advance(&mut self, p: &mut impl Perform, byte: u8) {
        if self.discard || byte == 0x18 || byte == 0x1a {
            self.discard = false;
            self.state = State::Ground;
            self.utf8.reset();

            p.cancel();
            return;
        }

        if !self.state.is_escape() {
            if self.utf8.remaining != 0 && !self.utf8_continue(p, byte) {
                return;
            }
            if (0xc2..=0xf4).contains(&byte) {
                self.state = State::Ground;
                self.utf8.start(byte);
                return;
            }
        }

        if self.state.is_escape() {
            self.escape_advance(p, byte);
            return;
        }

        if let State::CharsetSelect { g } = self.state {
            if byte <= 0x1f || byte == 0x7f {
                self.state = State::Ground;
            } else {
                p.designate_charset(g, byte);
                self.state = State::Ground;
                return;
            }
        }

        self.ground(p, byte);
    }

    fn utf8_continue(&mut self, p: &mut impl Perform, c: u8) -> bool {
        if (c & 0xc0) != 0x80 {
            let already_errored = self.utf8.cp > 0x10ffff;
            self.utf8.reset();
            if !already_errored {
                p.print_error(false);
            }
            return true;
        }

        self.utf8.remaining -= 1;
        self.utf8.cp |= ((c & 0x3f) as u64) << (6 * self.utf8.remaining as u32);

        if self.utf8.cp > 0x10ffff {
            return false;
        }
        if (self.utf8.remaining == 1 && self.utf8.cp < 0x800)
            || (self.utf8.remaining == 2 && self.utf8.cp < 0x10000)
        {
            p.print_error(true);
            self.utf8.cp = u64::MAX;
            return false;
        }
        if self.utf8.remaining != 0 {
            return false;
        }
        if (0xd800..=0xdfff).contains(&self.utf8.cp) {
            return true;
        }
        p.print_unicode(self.utf8.cp as u32);
        false
    }

    fn escape_advance(&mut self, p: &mut impl Perform, c: u8) {
        match self.state {
            State::Osc | State::OscEsc => self.osc_advance(p, c),
            State::CsiIgnore => self.csi_ignore(p, c),
            State::CsiEntry => self.csi_entry(p, c),
            State::CsiParam => self.csi_byte(p, c),
            State::Escape => self.escape_final(p, c),
            State::Ground | State::CharsetSelect { .. } => unreachable!(),
        }
    }

    fn osc_advance(&mut self, p: &mut impl Perform, c: u8) {
        match self.state {
            State::OscEsc => {
                if c == b'\\' {
                    self.state = State::Ground;
                } else {
                    self.state = State::Escape;
                    self.escape_final(p, c);
                }
            }
            State::Osc => match c {
                0x1b => self.state = State::OscEsc,
                0x07 => self.state = State::Ground,
                _ => {}
            },
            _ => unreachable!(),
        }
    }

    fn escape_final(&mut self, p: &mut impl Perform, c: u8) {
        match c {
            0x1b => {}
            b']' => self.state = State::Osc,
            b'[' => {
                self.params.clear();
                self.partial = false;
                self.private = false;
                self.discard = false;

                self.state = State::CsiEntry;
            }
            b'7' | b'8' | b'D' | b'E' | b'M' => {
                p.esc_dispatch(c);
                self.state = State::Ground;
            }
            b'c' => {
                p.ris();
                *self = Parser::default();
            }
            b'(' | b')' => self.state = State::CharsetSelect { g: c - b'(' },
            _ => self.state = State::Ground,
        }
    }

    fn csi_entry(&mut self, p: &mut impl Perform, c: u8) {
        match c {
            b'[' => {
                self.discard = true;
                self.state = State::Ground;
            }
            b'?' => {
                self.private = true;
                self.state = State::CsiParam;
            }
            _ => {
                self.state = State::CsiParam;
                self.csi_byte(p, c);
            }
        }
    }

    fn csi_byte(&mut self, p: &mut impl Perform, c: u8) {
        if c < 0x20 && c != 0x1b {
            p.execute(c, true);
            return;
        }
        if c.is_ascii_digit() {
            if !self.params.is_full() {
                self.partial = true;
                self.params.accumulate(c);
            }
            return;
        }
        if self.partial {
            self.params.commit();
            self.partial = false;
            if c == b';' {
                return;
            }
        } else if c == b';' {
            self.params.push_empty();
            return;
        }
        if self.private {
            if (0x20..=0x2f).contains(&c) {
                self.private = false;
                self.state = State::CsiIgnore;
                return;
            }
            p.csi_dispatch(c, &self.params, true);
            self.private = false;
            self.state = State::Ground;
            return;
        }
        if c == 0x1b {
            self.state = State::Escape;
            return;
        }
        if (0x40..=0x7e).contains(&c) {
            p.csi_dispatch(c, &self.params, false);
            self.state = State::Ground;
        } else {
            self.state = State::CsiIgnore;
        }
    }

    fn csi_ignore(&mut self, p: &mut impl Perform, c: u8) {
        if c == 0x1b {
            self.state = State::Escape;
        } else if c < 0x20 {
            p.execute(c, true);
        } else if (0x40..=0x7e).contains(&c) {
            self.state = State::Ground;
        }
    }

    fn ground(&mut self, p: &mut impl Perform, c: u8) {
        match c {
            0x00 | 0x7f => p.execute(c, false),
            0x1b => self.state = State::Escape,
            _ if c < 0x20 => p.execute(c, false),
            _ => p.print_byte(c),
        }
    }
}
