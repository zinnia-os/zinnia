use crate::uapi::input::*;

// HID usage pages.
pub const PAGE_GENERIC_DESKTOP: u32 = 0x01;
pub const PAGE_KEYBOARD_KEYPAD: u32 = 0x07;
pub const PAGE_BUTTON: u32 = 0x09;

// Generic desktop usages.
pub const GD_POINTER: u32 = 0x01;
pub const GD_MOUSE: u32 = 0x02;
pub const GD_JOYSTICK: u32 = 0x04;
pub const GD_GAMEPAD: u32 = 0x05;
pub const GD_KEYBOARD: u32 = 0x06;
pub const GD_KEYPAD: u32 = 0x07;
pub const GD_MULTI_AXIS_CONTROLLER: u32 = 0x08;
pub const GD_X: u32 = 0x30;
pub const GD_Y: u32 = 0x31;
pub const GD_Z: u32 = 0x32;
pub const GD_RX: u32 = 0x33;
pub const GD_RY: u32 = 0x34;
pub const GD_RZ: u32 = 0x35;
pub const GD_SLIDER: u32 = 0x36;
pub const GD_DIAL: u32 = 0x37;
pub const GD_WHEEL: u32 = 0x38;
pub const GD_HAT_SWITCH: u32 = 0x39;

pub const KEYBOARD_KEYPAD_MAX: u32 = 0xe7;

pub fn usage_page(usage: u32) -> u32 {
    (usage >> 16) & 0xffff
}

pub fn usage_code(usage: u32) -> u32 {
    usage & 0xffff
}

pub fn keyboard_to_key(hid: u32) -> u16 {
    match hid {
        0x04 => KEY_A,
        0x05 => KEY_B,
        0x06 => KEY_C,
        0x07 => KEY_D,
        0x08 => KEY_E,
        0x09 => KEY_F,
        0x0a => KEY_G,
        0x0b => KEY_H,
        0x0c => KEY_I,
        0x0d => KEY_J,
        0x0e => KEY_K,
        0x0f => KEY_L,
        0x10 => KEY_M,
        0x11 => KEY_N,
        0x12 => KEY_O,
        0x13 => KEY_P,
        0x14 => KEY_Q,
        0x15 => KEY_R,
        0x16 => KEY_S,
        0x17 => KEY_T,
        0x18 => KEY_U,
        0x19 => KEY_V,
        0x1a => KEY_W,
        0x1b => KEY_X,
        0x1c => KEY_Y,
        0x1d => KEY_Z,
        0x1e => KEY_1,
        0x1f => KEY_2,
        0x20 => KEY_3,
        0x21 => KEY_4,
        0x22 => KEY_5,
        0x23 => KEY_6,
        0x24 => KEY_7,
        0x25 => KEY_8,
        0x26 => KEY_9,
        0x27 => KEY_0,
        0x28 => KEY_ENTER,
        0x29 => KEY_ESC,
        0x2a => KEY_BACKSPACE,
        0x2b => KEY_TAB,
        0x2c => KEY_SPACE,
        0x2d => KEY_MINUS,
        0x2e => KEY_EQUAL,
        0x2f => KEY_LEFTBRACE,
        0x30 => KEY_RIGHTBRACE,
        0x31 => KEY_BACKSLASH,
        0x33 => KEY_SEMICOLON,
        0x34 => KEY_APOSTROPHE,
        0x35 => KEY_GRAVE,
        0x36 => KEY_COMMA,
        0x37 => KEY_DOT,
        0x38 => KEY_SLASH,
        0x39 => KEY_CAPSLOCK,
        0x3a => KEY_F1,
        0x3b => KEY_F2,
        0x3c => KEY_F3,
        0x3d => KEY_F4,
        0x3e => KEY_F5,
        0x3f => KEY_F6,
        0x40 => KEY_F7,
        0x41 => KEY_F8,
        0x42 => KEY_F9,
        0x43 => KEY_F10,
        0x44 => KEY_F11,
        0x45 => KEY_F12,
        0x47 => KEY_SCROLLLOCK,
        0x49 => KEY_INSERT,
        0x4a => KEY_HOME,
        0x4b => KEY_PAGEUP,
        0x4c => KEY_DELETE,
        0x4d => KEY_END,
        0x4e => KEY_PAGEDOWN,
        0x4f => KEY_RIGHT,
        0x50 => KEY_LEFT,
        0x51 => KEY_DOWN,
        0x52 => KEY_UP,
        0x53 => KEY_NUMLOCK,
        0x54 => KEY_KPSLASH,
        0x55 => KEY_KPASTERISK,
        0x56 => KEY_KPMINUS,
        0x57 => KEY_KPPLUS,
        0x58 => KEY_KPENTER,
        0x59 => KEY_KP1,
        0x5a => KEY_KP2,
        0x5b => KEY_KP3,
        0x5c => KEY_KP4,
        0x5d => KEY_KP5,
        0x5e => KEY_KP6,
        0x5f => KEY_KP7,
        0x60 => KEY_KP8,
        0x61 => KEY_KP9,
        0x62 => KEY_KP0,
        0x63 => KEY_KPDOT,
        0x68 => KEY_F13,
        0x69 => KEY_F14,
        0x6a => KEY_F15,
        0x6b => KEY_F16,
        0x6c => KEY_F17,
        0x6d => KEY_F18,
        0x6e => KEY_F19,
        0x6f => KEY_F20,
        0x70 => KEY_F21,
        0x71 => KEY_F22,
        0x72 => KEY_F23,
        0x73 => KEY_F24,
        0xe0 => KEY_LEFTCTRL,
        0xe1 => KEY_LEFTSHIFT,
        0xe2 => KEY_LEFTALT,
        0xe3 => KEY_LEFTMETA,
        0xe4 => KEY_RIGHTCTRL,
        0xe5 => KEY_RIGHTSHIFT,
        0xe6 => KEY_RIGHTALT,
        0xe7 => KEY_RIGHTMETA,
        _ => KEY_RESERVED,
    }
}

pub fn keyboard_is_modifier(usage: u32) -> bool {
    (0xe0..=0xe7).contains(&usage)
}

pub fn gd_to_rel(usage: u32) -> Option<u16> {
    Some(match usage {
        GD_X => REL_X,
        GD_Y => REL_Y,
        GD_WHEEL => REL_WHEEL,
        _ => return None,
    })
}

pub fn gd_to_abs(usage: u32) -> Option<u16> {
    Some(match usage {
        GD_X => ABS_X,
        GD_Y => ABS_Y,
        GD_Z => ABS_Z,
        GD_RX => ABS_RX,
        GD_RY => ABS_RY,
        GD_RZ => ABS_RZ,
        GD_SLIDER => ABS_THROTTLE,
        GD_DIAL => ABS_RUDDER,
        GD_WHEEL => ABS_WHEEL,
        _ => return None,
    })
}

pub fn mouse_button_to_key(usage: u32) -> u16 {
    match usage {
        1 => BTN_LEFT,
        2 => BTN_RIGHT,
        3 => BTN_MIDDLE,
        4 => BTN_SIDE,
        5 => BTN_EXTRA,
        6 => BTN_FORWARD,
        _ => KEY_RESERVED,
    }
}

pub fn gamepad_button_to_key(usage: u32) -> u16 {
    match usage {
        1 => BTN_SOUTH,
        2 => BTN_EAST,
        3 => BTN_C,
        4 => BTN_NORTH,
        5 => BTN_WEST,
        6 => BTN_Z,
        7 => BTN_TL,
        8 => BTN_TR,
        9 => BTN_TL2,
        10 => BTN_TR2,
        11 => BTN_SELECT,
        12 => BTN_START,
        13 => BTN_MODE,
        14 => BTN_THUMBL,
        15 => BTN_THUMBR,
        _ => KEY_RESERVED,
    }
}

pub fn joystick_button_to_key(usage: u32) -> u16 {
    match usage {
        1 => BTN_TRIGGER,
        2 => BTN_THUMB,
        3 => BTN_THUMB2,
        4 => BTN_TOP,
        5 => BTN_TOP2,
        6 => BTN_PINKIE,
        7 => BTN_BASE,
        8 => BTN_BASE2,
        9 => BTN_BASE3,
        10 => BTN_BASE4,
        11 => BTN_BASE5,
        12 => BTN_BASE6,
        _ => KEY_RESERVED,
    }
}
