use super::color::{Cell, CellColor, Ink, Palette, Pen, PixelFormat};
use super::parser::{Params, Parser, Perform};
use super::unicode::{cp437, wcwidth};
use alloc::{vec, vec::Vec};

const FONT_GLYPHS: usize = 256;

/// Active graphics charset for a designated slot.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Charset {
    Default,
    DecSpecial,
}

/// A pending cell change which is blitted on the next [`Term::flush`].
#[derive(Clone, Copy)]
struct QueueItem {
    x: usize,
    y: usize,
    cell: Cell,
}

/// A snapshot of the cursor/pen/charset state.
#[derive(Clone, Copy)]
struct SavedState {
    pen: Pen,
    cursor: (usize, usize),
    origin_mode: bool,
    charsets: [Charset; 2],
    current_charset: usize,
}

pub(super) struct TermConfig<'a> {
    pub fb: *mut u32,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub format: PixelFormat,
    pub font: &'a [u8],
    pub font_width: usize,
    pub font_height: usize,
}

pub struct Term {
    // Framebuffer.
    fb: *mut u32,
    pitch: usize,
    width: usize,
    height: usize,
    format: PixelFormat,

    // Font.
    font_width: usize,
    font_height: usize,
    offset_x: usize,
    offset_y: usize,
    font_bool: Vec<bool>,

    // Palette.
    palette: Palette,

    // Grid + double buffer.
    rows: usize,
    cols: usize,
    grid: Vec<Cell>,
    queue: Vec<QueueItem>,
    map: Vec<Option<usize>>,

    // Current rendition + cursor.
    pen: Pen,
    cursor_x: usize,
    cursor_y: usize,
    old_cursor_x: usize,
    old_cursor_y: usize,

    // Saved states.
    saved: SavedState,
    saved_cursor: (usize, usize),

    // Terminal modes.
    tab_size: usize,
    cursor_enabled: bool,
    scroll_enabled: bool,
    wrap_enabled: bool,
    origin_mode: bool,
    insert_mode: bool,
    charsets: [Charset; 2],
    current_charset: usize,
    scroll_top_margin: usize,
    scroll_bottom_margin: usize,
    last_printed_char: u8,
    last_was_graphic: bool,

    // Byte-stream parser.
    parser: Parser,
}

/// # Safety
/// The framebuffer pointer is only accessed while the owning lock is held.
unsafe impl Send for Term {}

impl Term {
    pub fn new(cfg: TermConfig) -> Self {
        let palette = Palette::new(&cfg.format);

        // Expand the 1bpp VGA font to one bool per pixel.
        let mut font_bool = vec![false; FONT_GLYPHS * cfg.font_height * cfg.font_width];
        for i in 0..FONT_GLYPHS {
            for y in 0..cfg.font_height {
                let byte = cfg.font[i * cfg.font_height + y];
                for x in 0..cfg.font_width.min(8) {
                    let offset = i * cfg.font_height * cfg.font_width + y * cfg.font_width + x;
                    font_bool[offset] = (byte & (0x80 >> x)) != 0;
                }
            }
        }

        let cols = cfg.width / cfg.font_width;
        let rows = cfg.height / cfg.font_height;
        let offset_x = (cfg.width % cfg.font_width) / 2;
        let offset_y = (cfg.height % cfg.font_height) / 2;

        let pen = Pen::new(&palette);
        let empty = Cell {
            glyph: b' ',
            fg: pen.fg,
            bg: pen.bg,
        };
        let saved = SavedState {
            pen,
            cursor: (0, 0),
            origin_mode: false,
            charsets: [Charset::Default, Charset::DecSpecial],
            current_charset: 0,
        };

        let mut term = Self {
            fb: cfg.fb,
            pitch: cfg.pitch,
            width: cfg.width,
            height: cfg.height,
            format: cfg.format,
            font_width: cfg.font_width,
            font_height: cfg.font_height,
            offset_x,
            offset_y,
            font_bool,
            palette,
            rows,
            cols,
            grid: vec![empty; rows * cols],
            queue: Vec::with_capacity(rows * cols),
            map: vec![None; rows * cols],
            pen,
            cursor_x: 0,
            cursor_y: 0,
            old_cursor_x: 0,
            old_cursor_y: 0,
            saved,
            saved_cursor: (0, 0),
            tab_size: 8,
            cursor_enabled: true,
            scroll_enabled: true,
            wrap_enabled: true,
            origin_mode: false,
            insert_mode: false,
            charsets: [Charset::Default, Charset::DecSpecial],
            current_charset: 0,
            scroll_top_margin: 0,
            scroll_bottom_margin: rows,
            last_printed_char: b' ',
            last_was_graphic: false,
            parser: Parser::default(),
        };

        term.reinit();
        term.full_refresh();
        term
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Feed one byte through the parser.
    pub fn putchar(&mut self, c: u8) {
        let mut parser = core::mem::take(&mut self.parser);
        parser.advance(self, c);
        self.parser = parser;
    }

    /// Reset the terminal-owned half of the state.
    fn reinit(&mut self) {
        self.tab_size = 8;
        self.cursor_enabled = true;
        self.scroll_enabled = true;
        self.wrap_enabled = true;
        self.origin_mode = false;
        self.insert_mode = false;
        self.pen.bold = false;
        self.pen.bg_bold = false;
        self.pen.reverse = false;
        self.pen.primary = Ink::Default;
        self.pen.bg_ink = Ink::Default;
        self.charsets = [Charset::Default, Charset::DecSpecial];
        self.current_charset = 0;
        self.saved_cursor = (0, 0);
        self.last_printed_char = b' ';
        self.last_was_graphic = false;
        self.scroll_top_margin = 0;
        self.scroll_bottom_margin = self.rows;
    }

    fn empty_char(&self) -> Cell {
        Cell {
            glyph: b' ',
            fg: self.pen.fg,
            bg: self.pen.bg,
        }
    }

    fn plot_char(&self, cell: Cell, x: usize, y: usize) {
        if x >= self.cols || y >= self.rows {
            return;
        }
        let resolve = |c: CellColor| match c {
            CellColor::Default => self.palette.default_bg,
            CellColor::Explicit(px) => px,
        };
        let bg = resolve(cell.bg);
        let fg = resolve(cell.fg);
        let px = self.offset_x + x * self.font_width;
        let py = self.offset_y + y * self.font_height;
        let stride = self.pitch / 4;
        let glyph_base = (cell.glyph as usize) * self.font_height * self.font_width;
        for gy in 0..self.font_height {
            let line = px + (py + gy) * stride;
            let gp = glyph_base + gy * self.font_width;
            for fx in 0..self.font_width {
                let colour = if self.font_bool[gp + fx] { fg } else { bg };
                // SAFETY: `line + fx` is within the mapped framebuffer.
                unsafe { self.fb.add(line + fx).write_volatile(colour) };
            }
        }
    }

    fn push_to_queue(&mut self, cell: Cell, x: usize, y: usize) {
        if x >= self.cols || y >= self.rows {
            return;
        }
        let i = y * self.cols + x;
        match self.map[i] {
            Some(qi) => self.queue[qi].cell = cell,
            None => {
                if self.grid[i] == cell {
                    return;
                }
                let qi = self.queue.len();
                self.queue.push(QueueItem { x, y, cell });
                self.map[i] = Some(qi);
            }
        }
    }

    fn cell_at(&self, i: usize) -> Cell {
        match self.map[i] {
            Some(qi) => self.queue[qi].cell,
            None => self.grid[i],
        }
    }

    fn draw_cursor(&mut self) {
        if self.cursor_x >= self.cols || self.cursor_y >= self.rows {
            return;
        }
        let i = self.cursor_x + self.cursor_y * self.cols;
        let mut cell = self.cell_at(i);
        core::mem::swap(&mut cell.fg, &mut cell.bg);
        self.plot_char(cell, self.cursor_x, self.cursor_y);
        if let Some(qi) = self.map[i] {
            self.grid[i] = self.queue[qi].cell;
            self.map[i] = None;
        }
    }

    pub fn flush(&mut self) {
        if self.cursor_enabled {
            self.draw_cursor();
        }
        for qi in 0..self.queue.len() {
            let q = self.queue[qi];
            let offset = q.y * self.cols + q.x;
            if self.map[offset].is_none() {
                continue;
            }
            self.plot_char(q.cell, q.x, q.y);
            self.grid[offset] = q.cell;
            self.map[offset] = None;
        }

        if ((self.old_cursor_x != self.cursor_x || self.old_cursor_y != self.cursor_y)
            || !self.cursor_enabled)
            && self.old_cursor_x < self.cols
            && self.old_cursor_y < self.rows
        {
            let g = self.grid[self.old_cursor_x + self.old_cursor_y * self.cols];
            self.plot_char(g, self.old_cursor_x, self.old_cursor_y);
        }

        self.old_cursor_x = self.cursor_x;
        self.old_cursor_y = self.cursor_y;
        self.queue.clear();
    }

    pub fn full_refresh(&mut self) {
        let stride = self.pitch / 4;
        for y in 0..self.height {
            for x in 0..self.width {
                // SAFETY: within the mapped framebuffer.
                unsafe {
                    self.fb
                        .add(y * stride + x)
                        .write_volatile(self.palette.default_bg)
                };
            }
        }
        for i in 0..self.rows * self.cols {
            let g = self.grid[i];
            self.plot_char(g, i % self.cols, i / self.cols);
        }
        if self.cursor_enabled {
            self.draw_cursor();
        }
    }

    fn scroll(&mut self) {
        let cols = self.cols;
        for i in (self.scroll_top_margin + 1) * cols..self.scroll_bottom_margin * cols {
            let cell = self.cell_at(i);
            self.push_to_queue(cell, i % cols, i / cols - 1);
        }
        let empty = self.empty_char();
        let last = self.scroll_bottom_margin - 1;
        for x in 0..cols {
            self.push_to_queue(empty, x, last);
        }
    }

    fn revscroll(&mut self) {
        let cols = self.cols;
        let start = self.scroll_top_margin * cols;
        let end = (self.scroll_bottom_margin - 1) * cols;
        let mut i = end;
        while i > start {
            i -= 1;
            let cell = self.cell_at(i);
            self.push_to_queue(cell, i % cols, i / cols + 1);
        }
        let empty = self.empty_char();
        let top = self.scroll_top_margin;
        for x in 0..cols {
            self.push_to_queue(empty, x, top);
        }
    }

    fn clear(&mut self, move_cursor: bool) {
        let empty = self.empty_char();
        for i in 0..self.rows * self.cols {
            self.push_to_queue(empty, i % self.cols, i / self.cols);
        }
        if move_cursor {
            self.cursor_x = 0;
            self.cursor_y = 0;
        }
    }

    fn set_cursor_pos(&mut self, mut x: usize, mut y: usize) {
        if x >= self.cols {
            x = if x > usize::MAX / 2 { 0 } else { self.cols - 1 };
        }
        if y >= self.rows {
            y = if y > usize::MAX / 2 { 0 } else { self.rows - 1 };
        }
        self.cursor_x = x;
        self.cursor_y = y;
    }

    fn get_cursor_pos(&self) -> (usize, usize) {
        (
            self.cursor_x.min(self.cols - 1),
            self.cursor_y.min(self.rows - 1),
        )
    }

    fn move_character(&mut self, new_x: usize, new_y: usize, old_x: usize, old_y: usize) {
        if old_x >= self.cols || old_y >= self.rows || new_x >= self.cols || new_y >= self.rows {
            return;
        }
        let cell = self.cell_at(old_x + old_y * self.cols);
        self.push_to_queue(cell, new_x, new_y);
    }

    fn raw_putchar(&mut self, c: u8) {
        if self.cursor_x >= self.cols {
            if self.wrap_enabled
                && (self.cursor_y < self.scroll_bottom_margin - 1 || self.scroll_enabled)
            {
                self.cursor_x = 0;
                self.cursor_y += 1;
                if self.cursor_y == self.scroll_bottom_margin {
                    self.cursor_y -= 1;
                    self.scroll();
                }
                if self.cursor_y >= self.rows {
                    self.cursor_y = self.rows - 1;
                }
            } else {
                self.cursor_x = self.cols - 1;
            }
        }
        let cell = Cell {
            glyph: c,
            fg: self.pen.fg,
            bg: self.pen.bg,
        };
        let (cx, cy) = (self.cursor_x, self.cursor_y);
        self.cursor_x += 1;
        self.push_to_queue(cell, cx, cy);
    }

    fn insert_shift(&mut self, mut count: usize) {
        if self.insert_mode && count > 0 {
            let (x, y) = self.get_cursor_pos();
            if count > self.cols - x {
                count = self.cols - x;
            }
            let mut i = self.cols - 1;
            while i >= x + count {
                self.move_character(i, y, i - count, y);
                i -= 1;
            }
        }
    }

    fn swap_palette(&mut self) {
        core::mem::swap(&mut self.pen.fg, &mut self.pen.bg);
    }

    fn set_text_color(&mut self, foreground: bool, color: CellColor) {
        if foreground {
            self.pen.fg = color;
        } else {
            self.pen.bg = color;
        }
    }

    fn set_palette_color(&mut self, foreground: bool, index: usize, bright: bool) {
        let palette = if bright {
            self.palette.bright[index]
        } else {
            self.palette.ansi[index]
        };
        self.set_text_color(foreground, CellColor::Explicit(palette));
    }

    fn set_default_fg(&mut self, bright: bool) {
        let color = if bright {
            self.palette.default_fg_bright
        } else {
            self.palette.default_fg
        };
        self.set_text_color(true, CellColor::Explicit(color));
    }

    fn set_default_bg(&mut self) {
        self.set_text_color(false, CellColor::Default);
    }

    fn set_default_bg_bright(&mut self) {
        self.set_text_color(false, CellColor::Explicit(self.palette.default_bg_bright));
    }

    fn set_rgb_color(&mut self, foreground: bool, rgb: u32) {
        self.set_text_color(foreground, CellColor::Explicit(self.format.convert(rgb)));
    }

    fn save_state(&mut self) {
        self.saved = SavedState {
            pen: self.pen,
            cursor: (self.cursor_x, self.cursor_y),
            origin_mode: self.origin_mode,
            charsets: self.charsets,
            current_charset: self.current_charset,
        };
    }

    fn restore_state(&mut self) {
        let s = self.saved;
        self.pen = s.pen;
        self.cursor_x = s.cursor.0;
        self.cursor_y = s.cursor.1;
        self.origin_mode = s.origin_mode;
        self.charsets = s.charsets;
        self.current_charset = s.current_charset;
    }

    fn sgr_reset(&mut self) {
        if self.pen.reverse {
            self.pen.reverse = false;
            self.swap_palette();
        }
        self.pen.bold = false;
        self.pen.bg_bold = false;
        self.pen.primary = Ink::Default;
        self.pen.bg_ink = Ink::Default;
        self.set_default_bg();
        self.set_default_fg(false);
    }

    fn sgr_set_fg(&mut self, idx: usize) {
        if (self.pen.bold && !self.pen.reverse) || (self.pen.bg_bold && self.pen.reverse) {
            self.set_palette_color(true, idx, true);
        } else {
            self.set_palette_color(true, idx, false);
        }
    }

    fn sgr_set_bg(&mut self, idx: usize) {
        if (self.pen.bold && self.pen.reverse) || (self.pen.bg_bold && !self.pen.reverse) {
            self.set_palette_color(false, idx, true);
        } else {
            self.set_palette_color(false, idx, false);
        }
    }

    fn sgr(&mut self, params: &Params) {
        let n = params.len();
        if n == 0 {
            self.sgr_reset();
            return;
        }
        let mut i = 0;
        while i < n {
            let v = params.get(i, 0);
            match v {
                0 => self.sgr_reset(),
                1 => {
                    self.pen.bold = true;
                    match self.pen.primary {
                        Ink::Rgb => {}
                        Ink::Indexed(idx) => {
                            if !self.pen.reverse {
                                self.set_palette_color(true, idx as usize, true);
                            } else {
                                self.set_palette_color(false, idx as usize, true);
                            }
                        }
                        Ink::Default => {
                            if !self.pen.reverse {
                                self.set_default_fg(true);
                            } else {
                                self.set_default_bg_bright();
                            }
                        }
                    }
                }
                5 => {
                    self.pen.bg_bold = true;
                    match self.pen.bg_ink {
                        Ink::Rgb => {}
                        Ink::Indexed(idx) => {
                            if !self.pen.reverse {
                                self.set_palette_color(false, idx as usize, true);
                            } else {
                                self.set_palette_color(true, idx as usize, true);
                            }
                        }
                        Ink::Default => {
                            if !self.pen.reverse {
                                self.set_default_bg_bright();
                            } else {
                                self.set_default_fg(true);
                            }
                        }
                    }
                }
                22 => {
                    self.pen.bold = false;
                    match self.pen.primary {
                        Ink::Rgb => {}
                        Ink::Indexed(idx) => {
                            if !self.pen.reverse {
                                self.set_palette_color(true, idx as usize, false);
                            } else {
                                self.set_palette_color(false, idx as usize, false);
                            }
                        }
                        Ink::Default => {
                            if !self.pen.reverse {
                                self.set_default_fg(false);
                            } else {
                                self.set_default_bg();
                            }
                        }
                    }
                }
                25 => {
                    self.pen.bg_bold = false;
                    match self.pen.bg_ink {
                        Ink::Rgb => {}
                        Ink::Indexed(idx) => {
                            if !self.pen.reverse {
                                self.set_palette_color(false, idx as usize, false);
                            } else {
                                self.set_palette_color(true, idx as usize, false);
                            }
                        }
                        Ink::Default => {
                            if !self.pen.reverse {
                                self.set_default_bg();
                            } else {
                                self.set_default_fg(false);
                            }
                        }
                    }
                }
                7 => {
                    if !self.pen.reverse {
                        self.pen.reverse = true;
                        self.swap_palette();
                    }
                }
                27 => {
                    if self.pen.reverse {
                        self.pen.reverse = false;
                        self.swap_palette();
                    }
                }
                30..=37 => {
                    let idx = (v - 30) as usize;
                    self.pen.primary = Ink::Indexed(idx as u8);
                    if self.pen.reverse {
                        self.sgr_set_bg(idx);
                    } else {
                        self.sgr_set_fg(idx);
                    }
                }
                40..=47 => {
                    let idx = (v - 40) as usize;
                    self.pen.bg_ink = Ink::Indexed(idx as u8);
                    if self.pen.reverse {
                        self.sgr_set_fg(idx);
                    } else {
                        self.sgr_set_bg(idx);
                    }
                }
                90..=97 => {
                    let idx = (v - 90) as usize;
                    self.pen.primary = Ink::Indexed(idx as u8);
                    if self.pen.reverse {
                        self.set_palette_color(false, idx, true);
                    } else {
                        self.set_palette_color(true, idx, true);
                    }
                }
                100..=107 => {
                    let idx = (v - 100) as usize;
                    self.pen.bg_ink = Ink::Indexed(idx as u8);
                    if self.pen.reverse {
                        self.set_palette_color(true, idx, true);
                    } else {
                        self.set_palette_color(false, idx, true);
                    }
                }
                39 => {
                    self.pen.primary = Ink::Default;
                    if self.pen.reverse {
                        self.swap_palette();
                    }
                    if !self.pen.bold {
                        self.set_default_fg(false);
                    } else {
                        self.set_default_fg(true);
                    }
                    if self.pen.reverse {
                        self.swap_palette();
                    }
                }
                49 => {
                    self.pen.bg_ink = Ink::Default;
                    if self.pen.reverse {
                        self.swap_palette();
                    }
                    if !self.pen.bg_bold {
                        self.set_default_bg();
                    } else {
                        self.set_default_bg_bright();
                    }
                    if self.pen.reverse {
                        self.swap_palette();
                    }
                }
                38 | 48 => {
                    let fg = v == 38;
                    let render_fg = if self.pen.reverse { !fg } else { fg };
                    i += 1;
                    if i >= n {
                        break;
                    }
                    match params.get(i, 0) {
                        2 => {
                            if i + 3 >= n {
                                return;
                            }
                            let rgb = ((params.get(i + 1, 0) & 0xff) << 16)
                                | ((params.get(i + 2, 0) & 0xff) << 8)
                                | (params.get(i + 3, 0) & 0xff);
                            i += 3;
                            if fg {
                                self.pen.primary = Ink::Rgb;
                            } else {
                                self.pen.bg_ink = Ink::Rgb;
                            }
                            if render_fg {
                                self.set_rgb_color(true, rgb);
                            } else {
                                self.set_rgb_color(false, rgb);
                            }
                        }
                        5 => {
                            if i + 1 >= n {
                                return;
                            }
                            let col = params.get(i + 1, 0);
                            i += 1;
                            if col < 8 {
                                self.set_ink(fg, Ink::Rgb);
                                if render_fg {
                                    self.set_palette_color(true, col as usize, false);
                                } else {
                                    self.set_palette_color(false, col as usize, false);
                                }
                            } else if col < 16 {
                                self.set_ink(fg, Ink::Rgb);
                                if render_fg {
                                    self.set_palette_color(true, (col - 8) as usize, true);
                                } else {
                                    self.set_palette_color(false, (col - 8) as usize, true);
                                }
                            } else if col < 256 {
                                self.set_ink(fg, Ink::Rgb);
                                let rgb = super::tables::COL256[(col - 16) as usize];
                                if render_fg {
                                    self.set_rgb_color(true, rgb);
                                } else {
                                    self.set_rgb_color(false, rgb);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn set_ink(&mut self, fg: bool, ink: Ink) {
        if fg {
            self.pen.primary = ink;
        } else {
            self.pen.bg_ink = ink;
        }
    }

    fn dec_private_parse(&mut self, c: u8, params: &Params) {
        if params.len() == 0 {
            return;
        }
        let set = match c {
            b'h' => true,
            b'l' => false,
            _ => return,
        };
        for i in 0..params.len() {
            match params.get(i, 1) {
                6 => {
                    self.origin_mode = set;
                    let y = if set { self.scroll_top_margin } else { 0 };
                    self.set_cursor_pos(0, y);
                }
                7 => self.wrap_enabled = set,
                25 => self.cursor_enabled = set,
                1049 => {
                    if set {
                        self.save_state();
                        self.clear(true);
                    } else {
                        self.clear(true);
                        self.restore_state();
                    }
                }
                _ => {}
            }
        }
    }

    fn mode_toggle(&mut self, c: u8, params: &Params) {
        if params.len() == 0 {
            return;
        }
        let set = match c {
            b'h' => true,
            b'l' => false,
            _ => return,
        };
        if params.get(0, 1) == 4 {
            self.insert_mode = set;
        }
    }

    fn erase_chars_block(&mut self, x: usize, y: usize, requested: usize) {
        let (cx, cy) = self.get_cursor_pos();
        self.set_cursor_pos(cx, cy);
        let count = requested.min(self.cols - cx);
        for _ in 0..count {
            self.raw_putchar(b' ');
        }
        self.set_cursor_pos(x, y);
    }

    fn execute_c0(&mut self, c: u8) {
        match c {
            0x08 => {
                let (x, y) = self.get_cursor_pos();
                if x > 0 {
                    self.set_cursor_pos(x - 1, y);
                }
            }
            0x09 => {
                let (mut x, y) = self.get_cursor_pos();
                x = (x / self.tab_size + 1) * self.tab_size;
                if x >= self.cols {
                    x = self.cols - 1;
                }
                self.set_cursor_pos(x, y);
            }
            0x0a..=0x0c => {
                let (x, y) = self.get_cursor_pos();
                if y == self.scroll_bottom_margin - 1 {
                    self.scroll();
                    self.set_cursor_pos(x, y);
                } else if y < self.rows - 1 {
                    self.set_cursor_pos(x, y + 1);
                }
            }
            0x0d => {
                let (_, y) = self.get_cursor_pos();
                self.set_cursor_pos(0, y);
            }
            14 => self.current_charset = 1,
            15 => self.current_charset = 0,
            _ => {}
        }
    }

    fn dec_special_print(&mut self, c: u8) -> bool {
        let mapped: u8 = match c {
            b'`' => 0x04,
            b'0' => 0xdb,
            b'-' => 0x18,
            b',' => 0x1b,
            b'.' => 0x19,
            b'a' => 0xb1,
            b'f' => 0xf8,
            b'g' => 0xf1,
            b'h' => 0xb0,
            b'j' => 0xd9,
            b'k' => 0xbf,
            b'l' => 0xda,
            b'm' => 0xc0,
            b'n' => 0xc5,
            b'q' => 0xc4,
            b's' => 0x5f,
            b't' => 0xc3,
            b'u' => 0xb4,
            b'v' => 0xc1,
            b'w' => 0xc2,
            b'x' => 0xb3,
            b'y' => 0xf3,
            b'z' => 0xf2,
            b'~' => 0xfa,
            b'_' => 0xff,
            b'+' => 0x1a,
            b'{' => 0xe3,
            b'}' => 0x9c,
            _ => return false,
        };
        self.last_printed_char = mapped;
        self.last_was_graphic = true;
        self.raw_putchar(mapped);
        true
    }

    fn csi_cursor_ops(&mut self, c: u8, params: &Params) {
        let saved_scroll = self.scroll_enabled;
        self.scroll_enabled = false;
        let saved_wrap = self.wrap_enabled;
        self.wrap_enabled = true;

        let (mut x, y) = self.get_cursor_pos();
        let default: u32 = if matches!(c, b'J' | b'K') { 0 } else { 1 };
        let ev0 = params.get(0, default) as usize;

        match c {
            b'F' | b'A' => {
                if c == b'F' {
                    x = 0;
                }
                let mut dest_y = y - ev0.min(y);
                let min_y = if self.origin_mode {
                    self.scroll_top_margin
                } else {
                    0
                };
                if dest_y < min_y {
                    dest_y = min_y;
                }
                self.set_cursor_pos(x, dest_y);
            }
            b'E' | b'e' | b'B' => {
                if c == b'E' {
                    x = 0;
                }
                let step = if y + ev0 > self.rows - 1 {
                    self.rows - 1 - y
                } else {
                    ev0
                };
                let mut dest_y = y + step;
                let max_y = if self.origin_mode {
                    self.scroll_bottom_margin
                } else {
                    self.rows
                };
                if dest_y >= max_y {
                    dest_y = max_y - 1;
                }
                self.set_cursor_pos(x, dest_y);
            }
            b'a' | b'C' => {
                let step = if x + ev0 > self.cols - 1 {
                    self.cols - 1 - x
                } else {
                    ev0
                };
                self.set_cursor_pos(x + step, y);
            }
            b'D' => {
                self.set_cursor_pos(x - ev0.min(x), y);
            }
            b'd' => {
                let mut row = ev0.saturating_sub(1);
                let (max_row, row_offset) = if self.origin_mode {
                    (
                        self.scroll_bottom_margin - self.scroll_top_margin,
                        self.scroll_top_margin,
                    )
                } else {
                    (self.rows, 0)
                };
                if row >= max_row {
                    row = max_row - 1;
                }
                self.set_cursor_pos(x, row + row_offset);
            }
            b'G' | b'`' => {
                let mut col = ev0.saturating_sub(1);
                if col >= self.cols {
                    col = self.cols - 1;
                }
                self.set_cursor_pos(col, y);
            }
            b'H' | b'f' => {
                let mut row = ev0.saturating_sub(1);
                let e1 = params.get(1, 1) as usize;
                let mut col = if e1 != 0 { e1 - 1 } else { 0 };
                let (max_row, row_offset) = if self.origin_mode {
                    (
                        self.scroll_bottom_margin - self.scroll_top_margin,
                        self.scroll_top_margin,
                    )
                } else {
                    (self.rows, 0)
                };
                if col >= self.cols {
                    col = self.cols - 1;
                }
                if row >= max_row {
                    row = max_row - 1;
                }
                self.set_cursor_pos(col, row + row_offset);
            }
            b'M' => {
                if y >= self.scroll_top_margin && y < self.scroll_bottom_margin {
                    let old = self.scroll_top_margin;
                    self.scroll_top_margin = y;
                    let count = ev0.min(self.scroll_bottom_margin - y);
                    for _ in 0..count {
                        self.scroll();
                    }
                    self.scroll_top_margin = old;
                }
            }
            b'L' => {
                if y >= self.scroll_top_margin && y < self.scroll_bottom_margin {
                    let old = self.scroll_top_margin;
                    self.scroll_top_margin = y;
                    let count = ev0.min(self.scroll_bottom_margin - y);
                    for _ in 0..count {
                        self.revscroll();
                    }
                    self.scroll_top_margin = old;
                }
            }
            b'J' => match ev0 {
                0 => {
                    self.set_cursor_pos(x, y);
                    for _ in x..self.cols {
                        self.raw_putchar(b' ');
                    }
                    for yc in (y + 1)..self.rows {
                        self.set_cursor_pos(0, yc);
                        for _ in 0..self.cols {
                            self.raw_putchar(b' ');
                        }
                    }
                    self.set_cursor_pos(x, y);
                }
                1 => {
                    for yc in 0..y {
                        self.set_cursor_pos(0, yc);
                        for _ in 0..self.cols {
                            self.raw_putchar(b' ');
                        }
                    }
                    self.set_cursor_pos(0, y);
                    for _ in 0..=x {
                        self.raw_putchar(b' ');
                    }
                    self.set_cursor_pos(x, y);
                }
                2 | 3 => self.clear(false),
                _ => {}
            },
            b'@' => {
                let mut nn = ev0;
                if nn != 0 {
                    if nn > self.cols - x {
                        nn = self.cols - x;
                    }
                    let mut i = self.cols - 1;
                    while i >= x + nn {
                        self.move_character(i, y, i - nn, y);
                        i -= 1;
                    }
                    self.set_cursor_pos(x, y);
                    for _ in 0..nn {
                        self.raw_putchar(b' ');
                    }
                    self.set_cursor_pos(x, y);
                }
            }
            b'P' => {
                let n = ev0.min(self.cols - x);
                for i in (x + n)..self.cols {
                    self.move_character(i - n, y, i, y);
                }
                self.set_cursor_pos(self.cols - n, y);
                self.erase_chars_block(x, y, n);
            }
            b'X' => {
                self.erase_chars_block(x, y, ev0);
            }
            b's' => {
                let (cx, cy) = self.get_cursor_pos();
                self.saved_cursor = (cx, cy);
            }
            b'u' => {
                self.set_cursor_pos(self.saved_cursor.0, self.saved_cursor.1);
            }
            b'K' => match ev0 {
                0 => {
                    self.set_cursor_pos(x, y);
                    for _ in x..self.cols {
                        self.raw_putchar(b' ');
                    }
                    self.set_cursor_pos(x, y);
                }
                1 => {
                    self.set_cursor_pos(0, y);
                    for _ in 0..=x {
                        self.raw_putchar(b' ');
                    }
                    self.set_cursor_pos(x, y);
                }
                2 => {
                    self.set_cursor_pos(0, y);
                    for _ in 0..self.cols {
                        self.raw_putchar(b' ');
                    }
                    self.set_cursor_pos(x, y);
                }
                _ => {}
            },
            b'r' => {
                self.scroll_top_margin = 0;
                self.scroll_bottom_margin = self.rows;
                if params.len() > 0 {
                    self.scroll_top_margin = params.get(0, 1) as usize - 1;
                }
                if params.len() > 1 {
                    self.scroll_bottom_margin = params.get(1, 1) as usize;
                }
                if self.scroll_top_margin >= self.rows
                    || self.scroll_bottom_margin > self.rows
                    || self.scroll_top_margin >= self.scroll_bottom_margin - 1
                {
                    self.scroll_top_margin = 0;
                    self.scroll_bottom_margin = self.rows;
                }
                let ny = if self.origin_mode {
                    self.scroll_top_margin
                } else {
                    0
                };
                self.set_cursor_pos(0, ny);
            }
            b'l' | b'h' => self.mode_toggle(c, params),
            b'S' => {
                let region = self.scroll_bottom_margin - self.scroll_top_margin;
                for _ in 0..ev0.min(region) {
                    self.scroll();
                }
            }
            b'T' => {
                let region = self.scroll_bottom_margin - self.scroll_top_margin;
                for _ in 0..ev0.min(region) {
                    self.revscroll();
                }
            }
            b'b' => {
                if self.last_was_graphic {
                    self.scroll_enabled = saved_scroll;
                    self.wrap_enabled = saved_wrap;
                    for _ in 0..ev0.min(self.cols) {
                        if self.insert_mode {
                            let (ix, iy) = self.get_cursor_pos();
                            let mut j = self.cols - 1;
                            while j > ix {
                                self.move_character(j, iy, j - 1, iy);
                                j -= 1;
                            }
                        }
                        self.raw_putchar(self.last_printed_char);
                    }
                }
            }
            _ => {}
        }

        self.scroll_enabled = saved_scroll;
        self.wrap_enabled = saved_wrap;
    }
}

impl Perform for Term {
    fn print_byte(&mut self, c: u8) {
        self.insert_shift(1);
        if self.charsets[self.current_charset] == Charset::DecSpecial && self.dec_special_print(c) {
            return;
        }
        if (0x20..=0x7e).contains(&c) {
            self.last_printed_char = c;
            self.last_was_graphic = true;
            self.raw_putchar(c);
        } else if c >= 0x80 {
            self.last_printed_char = 0xfe;
            self.last_was_graphic = true;
            self.raw_putchar(0xfe);
        }
    }

    fn print_unicode(&mut self, cp: u32) {
        match cp437(cp) {
            Some(mapped) => {
                self.insert_shift(1);
                self.last_printed_char = mapped;
                self.last_was_graphic = true;
                self.raw_putchar(mapped);
            }
            None => {
                let w = wcwidth(cp);
                if w > 0 {
                    self.insert_shift(w as usize);
                    self.last_printed_char = 0xfe;
                    self.last_was_graphic = true;
                    self.raw_putchar(0xfe);
                }
                for _ in 1..w {
                    self.raw_putchar(b' ');
                }
            }
        }
    }

    fn print_error(&mut self, update_last: bool) {
        self.insert_shift(1);
        if update_last {
            self.last_printed_char = 0xfe;
            self.last_was_graphic = true;
        }
        self.raw_putchar(0xfe);
    }

    fn execute(&mut self, c: u8, in_csi: bool) {
        if !in_csi {
            self.last_was_graphic = false;
        }
        self.execute_c0(c);
    }

    fn csi_dispatch(&mut self, final_byte: u8, params: &Params, private: bool) {
        if private {
            self.dec_private_parse(final_byte, params);
            return;
        }
        if final_byte == b'm' {
            self.sgr(params);
            return;
        }
        self.csi_cursor_ops(final_byte, params);
    }

    fn esc_dispatch(&mut self, final_byte: u8) {
        let (x, y) = self.get_cursor_pos();
        match final_byte {
            b'7' => self.save_state(),
            b'8' => self.restore_state(),
            b'D' => {
                if y == self.scroll_bottom_margin - 1 {
                    self.scroll();
                    self.set_cursor_pos(x, y);
                } else if y < self.rows - 1 {
                    self.set_cursor_pos(x, y + 1);
                }
            }
            b'E' => {
                if y == self.scroll_bottom_margin - 1 {
                    self.scroll();
                    self.set_cursor_pos(0, y);
                } else if y < self.rows - 1 {
                    self.set_cursor_pos(0, y + 1);
                } else {
                    self.set_cursor_pos(0, y);
                }
            }
            b'M' => {
                if y == self.scroll_top_margin {
                    self.revscroll();
                    self.set_cursor_pos(x, y);
                } else if y > 0 {
                    self.set_cursor_pos(x, y - 1);
                }
            }
            _ => {}
        }
    }

    fn designate_charset(&mut self, g: u8, final_byte: u8) {
        match final_byte {
            b'B' => self.charsets[g as usize] = Charset::Default,
            b'0' => self.charsets[g as usize] = Charset::DecSpecial,
            _ => {}
        }
    }

    fn cancel(&mut self) {
        self.last_was_graphic = false;
    }

    fn ris(&mut self) {
        if self.pen.reverse {
            self.swap_palette();
        }
        self.reinit();
        self.set_default_bg();
        self.set_default_fg(false);
        self.clear(true);
        self.save_state();
    }
}
