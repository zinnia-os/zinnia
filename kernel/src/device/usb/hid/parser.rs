use super::usage::{
    self, GD_GAMEPAD, GD_HAT_SWITCH, GD_JOYSTICK, GD_KEYBOARD, GD_KEYPAD, GD_MOUSE,
    GD_MULTI_AXIS_CONTROLLER, GD_POINTER, KEYBOARD_KEYPAD_MAX, PAGE_BUTTON, PAGE_GENERIC_DESKTOP,
    PAGE_KEYBOARD_KEYPAD,
};
use crate::{device::input::EventDevice, uapi::input::*};
use alloc::{sync::Arc, vec, vec::Vec};

const FLAG_CONSTANT: u16 = 1 << 0;
const FLAG_VARIABLE: u16 = 1 << 1;
const FLAG_RELATIVE: u16 = 1 << 2;

const KEY_BYTES: usize = (KEY_CNT as usize).div_ceil(8);
const REL_BYTES: usize = (REL_CNT as usize).div_ceil(8);
const ABS_BYTES: usize = (ABS_CNT as usize).div_ceil(8);

#[derive(Clone, Copy, Default)]
pub struct Usage {
    pub usage: u32,
    pub minimum: u32,
    pub maximum: u32,
}

#[derive(Clone)]
pub struct Field {
    pub application_id: usize,
    pub usages: Vec<Usage>,
    pub logical_minimum: i32,
    pub logical_maximum: i32,
    pub flags: u16,
    pub bit_offset: usize,
    pub bit_size: usize,
    pub count: usize,
}

impl Field {
    fn new(app_id: usize, global: &Global, local: &Local, flags: u16, bit_offset: usize) -> Self {
        Self {
            application_id: app_id,
            usages: local.usages.clone(),
            logical_minimum: global.logical_minimum,
            logical_maximum: global.logical_maximum,
            flags,
            bit_offset,
            bit_size: global.report_size as usize,
            count: global.report_count as usize,
        }
    }

    fn signed_value(&self, mut value: u64) -> i64 {
        if self.logical_minimum >= 0 || self.bit_size == 0 || self.bit_size >= 64 {
            return value as i64;
        }
        let sign_bit = 1u64 << (self.bit_size - 1);
        if value & sign_bit != 0 {
            value |= !((1u64 << self.bit_size) - 1);
        }
        value as i64
    }

    fn do_input(&self, data: &[u8], app: &mut Application, events: &mut Vec<(u16, u16, i32)>) {
        if self.bit_size > 64 {
            return;
        }
        let mut usage: Option<Usage> = None;
        for i in 0..self.count {
            if i < self.usages.len() {
                usage = Some(self.usages[i]);
            }
            let Some(usage) = usage else {
                break;
            };
            let value = get_bits(data, self.bit_size, self.bit_offset + self.bit_size * i);
            app.emit_event(self, &usage, i, value, events);
        }
    }

    fn emit_generic_desktop(
        &self,
        usage: &Usage,
        idx: usize,
        value: u64,
        events: &mut Vec<(u16, u16, i32)>,
    ) {
        if self.flags & FLAG_CONSTANT != 0 {
            return;
        }
        let mut hid_usage = usage::usage_code(usage.usage);
        if usage.minimum != 0 || usage.maximum != 0 {
            hid_usage = usage.minimum + idx as u32;
            if hid_usage > usage.maximum {
                return;
            }
        }

        let relative = self.flags & FLAG_RELATIVE != 0;
        if !relative && hid_usage == GD_HAT_SWITCH {
            self.emit_hat_switch(value, events);
            return;
        }

        let signed = self.signed_value(value);
        if relative {
            if signed == 0 {
                return;
            }
            if let Some(code) = usage::gd_to_rel(hid_usage) {
                events.push((EV_REL, code, signed as i32));
            }
        } else if let Some(code) = usage::gd_to_abs(hid_usage) {
            events.push((EV_ABS, code, signed as i32));
        }
    }

    fn emit_hat_switch(&self, value: u64, events: &mut Vec<(u16, u16, i32)>) {
        const HAT: [(i32, i32); 8] = [
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -1),
        ];
        let hat = self.signed_value(value) - self.logical_minimum as i64;
        let (x, y) = if (0..8).contains(&hat) {
            HAT[hat as usize]
        } else {
            (0, 0)
        };
        events.push((EV_ABS, ABS_HAT0X, x));
        events.push((EV_ABS, ABS_HAT0Y, y));
    }
}

pub struct Report {
    pub report_id: u32,
    pub inputs: Vec<Field>,
    pub bit_size: usize,
}

pub struct Application {
    pub usage: u32,
    pub pressed_keys: [u64; 4],
    pub pressed_buttons: u64,
}

impl Application {
    fn new(usage: u32) -> Self {
        Self {
            usage,
            pressed_keys: [0; 4],
            pressed_buttons: 0,
        }
    }

    fn is_gamepad(&self) -> bool {
        usage::usage_page(self.usage) == PAGE_GENERIC_DESKTOP
            && usage::usage_code(self.usage) == GD_GAMEPAD
    }

    fn is_joystick(&self) -> bool {
        usage::usage_page(self.usage) == PAGE_GENERIC_DESKTOP
            && matches!(
                usage::usage_code(self.usage),
                GD_JOYSTICK | GD_MULTI_AXIS_CONTROLLER
            )
    }

    fn emit_event(
        &mut self,
        field: &Field,
        usage: &Usage,
        idx: usize,
        value: u64,
        events: &mut Vec<(u16, u16, i32)>,
    ) {
        match usage::usage_page(usage.usage) {
            PAGE_GENERIC_DESKTOP => field.emit_generic_desktop(usage, idx, value, events),
            PAGE_KEYBOARD_KEYPAD => self.emit_keyboard(field, usage, idx, value),
            PAGE_BUTTON => self.emit_button(field, usage, idx, value),
            _ => {}
        }
    }

    fn emit_keyboard(&mut self, field: &Field, usage: &Usage, idx: usize, value: u64) {
        if field.flags & FLAG_CONSTANT != 0 {
            return;
        }
        let key = if field.flags & FLAG_VARIABLE != 0 {
            if value == 0 {
                return;
            }
            if usage.minimum != 0 || usage.maximum != 0 {
                let v = usage.minimum + idx as u32;
                if v > usage.maximum {
                    return;
                }
                v
            } else {
                usage::usage_code(usage.usage)
            }
        } else {
            value as u32
        };
        hid_key_set(&mut self.pressed_keys, key);
    }

    fn hid_button_set(&mut self, button: u32) {
        if !(1..=16).contains(&button) {
            return;
        }
        self.pressed_buttons |= 1u64 << (button - 1);
    }

    fn emit_button(&mut self, field: &Field, usage: &Usage, idx: usize, value: u64) {
        if field.flags & FLAG_CONSTANT != 0 {
            return;
        }
        if field.flags & FLAG_VARIABLE != 0 {
            if value == 0 {
                return;
            }
            let button = if usage.minimum != 0 || usage.maximum != 0 {
                let b = usage.minimum + idx as u32;
                if b > usage.maximum {
                    return;
                }
                b
            } else {
                usage::usage_code(usage.usage)
            };
            self.hid_button_set(button);
        } else if value != 0 {
            self.hid_button_set(value as u32);
        }
    }

    fn button_to_key(&self, button: u32) -> u16 {
        if self.is_gamepad() {
            usage::gamepad_button_to_key(button)
        } else if self.is_joystick() {
            usage::joystick_button_to_key(button)
        } else {
            usage::mouse_button_to_key(button)
        }
    }

    fn handle_key_changes(
        &self,
        events: &mut Vec<(u16, u16, i32)>,
        old_keys: u64,
        offset: usize,
        modifiers: bool,
    ) {
        let mut changes = old_keys ^ self.pressed_keys[offset];
        while changes != 0 {
            let bit = changes.trailing_zeros();
            let value = offset as u32 * 64 + bit;
            if usage::keyboard_is_modifier(value) != modifiers {
                changes &= !(1u64 << bit);
                continue;
            }
            let code = usage::keyboard_to_key(value);
            if code != KEY_RESERVED {
                let pressed = self.pressed_keys[offset] & (1u64 << bit) != 0;
                events.push((EV_KEY, code, pressed as i32));
            }
            changes &= !(1u64 << bit);
        }
    }

    fn handle_button_changes(&self, events: &mut Vec<(u16, u16, i32)>, old_buttons: u64) {
        let mut changes = old_buttons ^ self.pressed_buttons;
        while changes != 0 {
            let bit = changes.trailing_zeros();
            let button = 1 + bit;
            let code = self.button_to_key(button);
            if code != KEY_RESERVED {
                let pressed = self.pressed_buttons & (1u64 << bit) != 0;
                events.push((EV_KEY, code, pressed as i32));
            }
            changes &= !(1u64 << bit);
        }
    }
}

pub struct Parser {
    /// Reports keyed by report id.
    pub reports: Vec<Report>,
    /// Anonymous inputs.
    pub inputs: Vec<Field>,
    pub input_bit_size: usize,
    pub applications: Vec<Application>,
}

/// Input capabilities.
pub struct Caps {
    pub ev: u32,
    pub keys: Vec<u8>,
    pub rels: Vec<u8>,
    pub abs: Vec<u8>,
    pub abs_info: Vec<(i32, i32)>,
    pub name: &'static str,
}

impl Caps {
    fn new() -> Self {
        Self {
            ev: 0,
            keys: vec![0u8; KEY_BYTES],
            rels: vec![0u8; REL_BYTES],
            abs: vec![0u8; ABS_BYTES],
            abs_info: vec![(0, 0); ABS_CNT as usize],
            name: "HID Device",
        }
    }

    fn advertise_usage(&mut self, app: &Application, field: &Field, usage: &Usage) {
        if field.flags & FLAG_CONSTANT != 0 {
            return;
        }
        match usage::usage_page(usage.usage) {
            PAGE_GENERIC_DESKTOP => self.advertise_generic_desktop(field, usage),
            PAGE_KEYBOARD_KEYPAD => {
                self.ev |= 1 << EV_KEY;
                self.advertise_key_range(usage, usage::keyboard_to_key);
            }
            PAGE_BUTTON => {
                self.ev |= 1 << EV_KEY;
                self.advertise_key_range(usage, |b| app.button_to_key(b));
            }
            _ => {}
        }
    }

    fn advertise_key_range(&mut self, usage: &Usage, map: impl Fn(u32) -> u16) {
        if usage.minimum == 0 && usage.maximum == 0 {
            let code = map(usage::usage_code(usage.usage));
            if code != KEY_RESERVED {
                set_bit(&mut self.keys, code);
            }
            return;
        }
        let mut u = usage.minimum;
        while u <= usage.maximum && u <= KEYBOARD_KEYPAD_MAX {
            let code = map(u);
            if code != KEY_RESERVED {
                set_bit(&mut self.keys, code);
            }
            u += 1;
        }
    }

    fn advertise_generic_desktop(&mut self, field: &Field, usage: &Usage) {
        let relative = field.flags & FLAG_RELATIVE != 0;
        let lmin = field.logical_minimum;
        let lmax = field.logical_maximum;
        let mut advertise_one = |hid_usage: u32| {
            if relative {
                if let Some(code) = usage::gd_to_rel(hid_usage) {
                    self.ev |= 1 << EV_REL;
                    set_bit(&mut self.rels, code);
                }
            } else if hid_usage == GD_HAT_SWITCH {
                self.ev |= 1 << EV_ABS;
                set_bit(&mut self.abs, ABS_HAT0X);
                set_bit(&mut self.abs, ABS_HAT0Y);
                self.abs_info[ABS_HAT0X as usize] = (-1, 1);
                self.abs_info[ABS_HAT0Y as usize] = (-1, 1);
            } else if let Some(code) = usage::gd_to_abs(hid_usage) {
                self.ev |= 1 << EV_ABS;
                set_bit(&mut self.abs, code);
                self.abs_info[code as usize] = (lmin, lmax);
            }
        };

        if usage.minimum == 0 && usage.maximum == 0 {
            advertise_one(usage::usage_code(usage.usage));
            return;
        }
        let mut u = usage.minimum;
        while u <= usage.maximum && u <= GD_HAT_SWITCH {
            advertise_one(u);
            u += 1;
        }
    }
}

fn set_bit(bitmap: &mut [u8], n: u16) {
    let byte = (n / 8) as usize;
    if byte < bitmap.len() {
        bitmap[byte] |= 1 << (n % 8);
    }
}

fn translate_size(size: u8) -> usize {
    match size {
        0 => 0,
        1 => 1,
        2 => 2,
        _ => 4,
    }
}

fn data_u(data: &[u8]) -> u32 {
    let mut v = 0u32;
    for (i, &b) in data.iter().take(4).enumerate() {
        v |= (b as u32) << (i * 8);
    }
    v
}

fn data_i(data: &[u8]) -> i32 {
    match data.len() {
        1 => data[0] as i8 as i32,
        2 => u16::from_le_bytes([data[0], data[1]]) as i16 as i32,
        4 => i32::from_le_bytes([data[0], data[1], data[2], data[3]]),
        _ => 0,
    }
}

#[derive(Default, Clone, Copy)]
struct Global {
    usage_page: u32,
    logical_minimum: i32,
    logical_maximum: i32,
    report_size: u32,
    report_id: u32,
    report_count: u32,
}

impl Global {
    fn handle_global(&mut self, tag: u8, item: &[u8]) -> Result<(), ()> {
        let udata = data_u(item);
        let idata = data_i(item);
        match tag {
            0x00 => self.usage_page = udata,
            0x01 => self.logical_minimum = idata,
            0x02 => self.logical_maximum = idata,
            0x07 => self.report_size = udata,
            0x08 => {
                if udata == 0 || udata > 0xff {
                    return Err(());
                }
                self.report_id = udata;
            }
            0x09 => self.report_count = udata,
            // Physical min/max, unit, push/pop and others ignored.
            _ => {}
        }
        Ok(())
    }

    fn handle_local(&self, local: &mut Local, tag: u8, item: &[u8], actual_size: usize) {
        let udata = data_u(item);
        match tag {
            0x00 => local
                .usages
                .push(make_usage(self.usage_page, udata, actual_size)),
            0x01 => {
                local.has_usage_minimum = true;
                local.usage_minimum = udata;
            }
            0x02 => {
                if local.has_usage_minimum {
                    local.usages.push(Usage {
                        usage: self.usage_page << 16,
                        minimum: local.usage_minimum,
                        maximum: udata,
                    });
                    local.has_usage_minimum = false;
                }
            }
            // Designator/string indices and delimiters ignored.
            _ => {}
        }
    }
}

#[derive(Default)]
struct Local {
    usages: Vec<Usage>,
    has_usage_minimum: bool,
    usage_minimum: u32,
}

impl Parser {
    pub fn parse(data: &[u8]) -> Result<Parser, ()> {
        let mut parser = Parser {
            reports: Vec::new(),
            inputs: Vec::new(),
            input_bit_size: 0,
            applications: Vec::new(),
        };
        let mut global = Global::default();
        let mut local = Local::default();
        let mut depth: usize = 0;

        let mut off = 0;
        while off < data.len() {
            let item = data[off];

            // Long item: 0xfe, bDataSize, bLongItemTag, then data. Skipped.
            if item == 0xfe {
                if off + 2 >= data.len() {
                    return Err(());
                }
                off += 3 + data[off + 1] as usize;
                continue;
            }

            let size = item & 0x3;
            let itype = (item >> 2) & 0x3;
            let tag = (item >> 4) & 0xf;
            let actual = translate_size(size);
            if off + 1 + actual > data.len() {
                return Err(());
            }
            let item_data = &data[off + 1..off + 1 + actual];

            match itype {
                0x01 => global.handle_global(tag, item_data)?,
                0x02 => global.handle_local(&mut local, tag, item_data, actual),
                0x00 => {
                    parser.handle_main(&mut depth, &global, &local, tag, item_data)?;
                    local = Local::default();
                }
                _ => {}
            }

            off += 1 + actual;
        }

        if depth != 0 {
            return Err(());
        }

        Ok(parser)
    }

    /// Decodes one interrupt report and emits events to `devices`.
    pub fn parse_report(&mut self, mut data: &[u8], devices: &[Arc<EventDevice>]) {
        let Self {
            reports,
            inputs,
            input_bit_size,
            applications,
        } = self;

        let report_idx = if !reports.is_empty() {
            if data.is_empty() {
                return;
            }
            let id = data[0] as u32;
            data = &data[1..];
            match reports.iter().position(|r| r.report_id == id) {
                Some(i) => Some(i),
                None => return,
            }
        } else {
            None
        };

        let needed_bits = match report_idx {
            Some(i) => reports[i].bit_size,
            None => *input_bit_size,
        };
        if needed_bits.div_ceil(8) > data.len() {
            return;
        }

        let input_list: &[Field] = match report_idx {
            Some(i) => &reports[i].inputs,
            None => inputs,
        };

        let app_count = applications.len();
        let mut events: Vec<Vec<(u16, u16, i32)>> = (0..app_count).map(|_| Vec::new()).collect();
        let mut old_keys = vec![[0u64; 4]; app_count];
        let mut old_buttons = vec![0u64; app_count];
        let mut handled = 0u64;

        for field in input_list {
            let app_id = field.application_id;
            // `handled` is a 64-bit mask. Ignore pathological descriptors.
            if app_id >= app_count || app_id >= 64 {
                continue;
            }
            if handled & (1 << app_id) == 0 {
                old_keys[app_id] = applications[app_id].pressed_keys;
                applications[app_id].pressed_keys = [0; 4];
                old_buttons[app_id] = applications[app_id].pressed_buttons;
                applications[app_id].pressed_buttons = 0;
                handled |= 1 << app_id;
            }
            field.do_input(data, &mut applications[app_id], &mut events[app_id]);
        }

        for app_id in 0..app_count.min(64) {
            if handled & (1 << app_id) == 0 {
                continue;
            }
            let app = &applications[app_id];
            let ev = &mut events[app_id];

            for j in 0..4 {
                if old_keys[app_id][j] != app.pressed_keys[j] {
                    app.handle_key_changes(ev, old_keys[app_id][j], j, true);
                }
            }
            for j in 0..4 {
                if old_keys[app_id][j] != app.pressed_keys[j] {
                    app.handle_key_changes(ev, old_keys[app_id][j], j, false);
                }
            }
            if old_buttons[app_id] != app.pressed_buttons {
                app.handle_button_changes(ev, old_buttons[app_id]);
            }

            if !ev.is_empty()
                && let Some(device) = devices.get(app_id)
            {
                for &(typ, code, value) in ev.iter() {
                    device.report_event(typ, code, value);
                }
                device.report_event(EV_SYN, SYN_REPORT, 0);
            }
        }
    }

    /// Builds input capabilities.
    pub fn build_caps(&self) -> Vec<Caps> {
        let mut caps: Vec<Caps> = (0..self.applications.len()).map(|_| Caps::new()).collect();

        for (i, app) in self.applications.iter().enumerate() {
            caps[i].name = match usage::usage_code(app.usage) {
                GD_POINTER => "HID Pointer",
                GD_MOUSE => "HID Mouse",
                GD_JOYSTICK => "HID Joystick",
                GD_GAMEPAD => "HID Gamepad",
                GD_KEYBOARD => "HID Keyboard",
                GD_KEYPAD => "HID Keypad",
                GD_MULTI_AXIS_CONTROLLER => "HID Multi Axis Controller",
                _ => "HID Device",
            };
        }

        let advertise = |caps: &mut [Caps], applications: &[Application], field: &Field| {
            let cap = &mut caps[field.application_id];
            let app = &applications[field.application_id];
            for usage in &field.usages {
                cap.advertise_usage(app, field, usage);
            }
        };

        if !self.reports.is_empty() {
            for report in &self.reports {
                for field in &report.inputs {
                    advertise(&mut caps, &self.applications, field);
                }
            }
        } else {
            for field in &self.inputs {
                advertise(&mut caps, &self.applications, field);
            }
        }

        caps
    }

    fn handle_main(
        &mut self,
        depth: &mut usize,
        global: &Global,
        local: &Local,
        tag: u8,
        item: &[u8],
    ) -> Result<(), ()> {
        let udata = data_u(item);
        match tag {
            0x08 => self.handle_input(*depth, global, local, udata as u16),
            0x0a => self.handle_collection(depth, local, udata as u8),
            0x0c => handle_collection_end(depth),
            // OUTPUT / FEATURE: ignored.
            _ => Ok(()),
        }
    }

    fn handle_collection(&mut self, depth: &mut usize, local: &Local, typ: u8) -> Result<(), ()> {
        const COLLECTION_APPLICATION: u8 = 0x01;
        if *depth == 0 && typ != COLLECTION_APPLICATION {
            return Err(());
        }
        let was = *depth;
        *depth += 1;
        if was != 0 {
            return Ok(());
        }
        if local.usages.len() != 1 {
            return Err(());
        }
        self.applications
            .push(Application::new(local.usages[0].usage));
        Ok(())
    }

    fn handle_input(
        &mut self,
        depth: usize,
        global: &Global,
        local: &Local,
        flags: u16,
    ) -> Result<(), ()> {
        if depth == 0 {
            return Err(());
        }
        let app_id = self.applications.len() - 1;

        if global.report_id == 0 {
            if !self.reports.is_empty() {
                return Err(());
            }
            let mut bit_size = self.input_bit_size;
            self.inputs
                .push(Field::new(app_id, global, local, flags, bit_size));
            bit_size += (global.report_size * global.report_count) as usize;
            self.input_bit_size = bit_size;
            return Ok(());
        }

        if !self.inputs.is_empty() {
            return Err(());
        }

        let idx = match self
            .reports
            .iter()
            .position(|r| r.report_id == global.report_id)
        {
            Some(i) => i,
            None => {
                self.reports.push(Report {
                    report_id: global.report_id,
                    inputs: Vec::new(),
                    bit_size: 0,
                });
                self.reports.len() - 1
            }
        };
        let report = &mut self.reports[idx];
        report
            .inputs
            .push(Field::new(app_id, global, local, flags, report.bit_size));
        report.bit_size += (global.report_size * global.report_count) as usize;
        Ok(())
    }
}

fn make_usage(current_page: u32, raw: u32, actual_size: usize) -> Usage {
    Usage {
        usage: if actual_size <= 2 {
            (current_page << 16) | raw
        } else {
            raw
        },
        minimum: 0,
        maximum: 0,
    }
}

fn handle_collection_end(depth: &mut usize) -> Result<(), ()> {
    if *depth == 0 {
        return Err(());
    }
    *depth -= 1;
    Ok(())
}

fn get_bits(data: &[u8], bit_count: usize, bit_offset: usize) -> u64 {
    let mut value = 0u64;
    if bit_count == 0 || bit_count > 64 {
        return 0;
    }
    for i in 0..bit_count {
        let src = bit_offset + i;
        let byte = src / 8;
        if byte >= data.len() {
            break;
        }
        let bit = (data[byte] >> (src % 8)) & 1;
        value |= (bit as u64) << i;
    }
    value
}

fn hid_key_set(keys: &mut [u64; 4], key: u32) {
    if key > KEYBOARD_KEYPAD_MAX {
        return;
    }
    keys[(key / 64) as usize] |= 1u64 << (key % 64);
}
