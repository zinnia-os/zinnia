use crate::{
    arch::x86_64::asm::{read8, write8},
    boot::BootInfo,
    device::input::{EventDevice, EventDeviceOps},
    irq::{IrqHandler, IrqLine, Status},
    uapi::input::*,
};
use alloc::{boxed::Box, sync::Arc};

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const CMD_PORT: u16 = 0x64;

const STATUS_OUTPUT_FULL: u8 = 1 << 0;
const STATUS_INPUT_FULL: u8 = 1 << 1;
const STATUS_MOUSE_DATA: u8 = 1 << 5;

const CMD_READ_CONFIG: u8 = 0x20;
const CMD_WRITE_CONFIG: u8 = 0x60;
const CMD_DISABLE_PORT2: u8 = 0xA7;
const CMD_ENABLE_PORT2: u8 = 0xA8;
const CMD_DISABLE_PORT1: u8 = 0xAD;
const CMD_ENABLE_PORT1: u8 = 0xAE;
const CMD_WRITE_PORT2: u8 = 0xD4;

const CFG_PORT1_IRQ: u8 = 1 << 0;
const CFG_PORT2_IRQ: u8 = 1 << 1;
#[allow(dead_code)]
const CFG_PORT1_CLOCK_DISABLE: u8 = 1 << 4;
#[allow(dead_code)]
const CFG_PORT2_CLOCK_DISABLE: u8 = 1 << 5;
const CFG_PORT1_TRANSLATION: u8 = 1 << 6;

const DEV_RESET: u8 = 0xFF;
const DEV_ENABLE_SCANNING: u8 = 0xF4;
const DEV_SET_DEFAULTS: u8 = 0xF6;

struct Ps2Controller;

impl Ps2Controller {
    fn init() {
        // Disable both ports during setup.
        Self::send_command(CMD_DISABLE_PORT1);
        Self::send_command(CMD_DISABLE_PORT2);
        Self::flush_output();

        // Read config byte, disable IRQs but keep translation enabled.
        Self::send_command(CMD_READ_CONFIG);
        let mut config = Self::read_data();
        config &= !(CFG_PORT1_IRQ | CFG_PORT2_IRQ);
        config |= CFG_PORT1_TRANSLATION;
        Self::send_command(CMD_WRITE_CONFIG);
        Self::send_data(config);

        // Enable ports.
        Self::send_command(CMD_ENABLE_PORT1);
        Self::send_command(CMD_ENABLE_PORT2);

        // Reset keyboard.
        Self::send_data(DEV_RESET);
        if Self::wait_output() {
            let _ = unsafe { read8(DATA_PORT) }; // ACK or self-test result
        }
        Self::flush_output();

        // Reset mouse.
        Self::send_mouse_byte(DEV_RESET);
        if Self::wait_output() {
            let _ = unsafe { read8(DATA_PORT) }; // ACK
        }
        // Mouse self-test sends 0xAA then device ID.
        if Self::wait_output() {
            let _ = unsafe { read8(DATA_PORT) };
        }
        if Self::wait_output() {
            let _ = unsafe { read8(DATA_PORT) };
        }

        // Enable scanning on both devices.
        Self::send_data(DEV_SET_DEFAULTS);
        if Self::wait_output() {
            let _ = unsafe { read8(DATA_PORT) };
        }
        Self::send_data(DEV_ENABLE_SCANNING);
        if Self::wait_output() {
            let _ = unsafe { read8(DATA_PORT) };
        }

        Self::send_mouse_byte(DEV_SET_DEFAULTS);
        if Self::wait_output() {
            let _ = unsafe { read8(DATA_PORT) };
        }
        Self::send_mouse_byte(DEV_ENABLE_SCANNING);
        if Self::wait_output() {
            let _ = unsafe { read8(DATA_PORT) };
        }

        Self::flush_output();
    }

    fn enable_irqs() {
        Self::flush_output();
        Self::send_command(CMD_READ_CONFIG);
        let mut config = Self::read_data();
        config |= CFG_PORT1_IRQ | CFG_PORT2_IRQ;
        Self::send_command(CMD_WRITE_CONFIG);
        Self::send_data(config);
    }

    fn wait_input() {
        for _ in 0..100_000 {
            if unsafe { read8(STATUS_PORT) } & STATUS_INPUT_FULL == 0 {
                return;
            }
            core::hint::spin_loop();
        }
    }

    fn wait_output() -> bool {
        for _ in 0..100_000 {
            if unsafe { read8(STATUS_PORT) } & STATUS_OUTPUT_FULL != 0 {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }

    fn send_command(cmd: u8) {
        Self::wait_input();
        unsafe { write8(CMD_PORT, cmd) };
    }

    fn send_data(data: u8) {
        Self::wait_input();
        unsafe { write8(DATA_PORT, data) };
    }

    fn read_data() -> u8 {
        Self::wait_output();
        unsafe { read8(DATA_PORT) }
    }

    fn flush_output() {
        for _ in 0..256 {
            if unsafe { read8(STATUS_PORT) } & STATUS_OUTPUT_FULL == 0 {
                break;
            }
            unsafe { read8(DATA_PORT) };
        }
    }

    fn send_mouse_byte(byte: u8) {
        Self::send_command(CMD_WRITE_PORT2);
        Self::send_data(byte);
    }
}

static SCANCODE_TABLE: [u16; 128] = {
    let mut t = [0u16; 128];
    t[0x01] = KEY_ESC;
    t[0x02] = KEY_1;
    t[0x03] = KEY_2;
    t[0x04] = KEY_3;
    t[0x05] = KEY_4;
    t[0x06] = KEY_5;
    t[0x07] = KEY_6;
    t[0x08] = KEY_7;
    t[0x09] = KEY_8;
    t[0x0A] = KEY_9;
    t[0x0B] = KEY_0;
    t[0x0C] = KEY_MINUS;
    t[0x0D] = KEY_EQUAL;
    t[0x0E] = KEY_BACKSPACE;
    t[0x0F] = KEY_TAB;
    t[0x10] = KEY_Q;
    t[0x11] = KEY_W;
    t[0x12] = KEY_E;
    t[0x13] = KEY_R;
    t[0x14] = KEY_T;
    t[0x15] = KEY_Y;
    t[0x16] = KEY_U;
    t[0x17] = KEY_I;
    t[0x18] = KEY_O;
    t[0x19] = KEY_P;
    t[0x1A] = KEY_LEFTBRACE;
    t[0x1B] = KEY_RIGHTBRACE;
    t[0x1C] = KEY_ENTER;
    t[0x1D] = KEY_LEFTCTRL;
    t[0x1E] = KEY_A;
    t[0x1F] = KEY_S;
    t[0x20] = KEY_D;
    t[0x21] = KEY_F;
    t[0x22] = KEY_G;
    t[0x23] = KEY_H;
    t[0x24] = KEY_J;
    t[0x25] = KEY_K;
    t[0x26] = KEY_L;
    t[0x27] = KEY_SEMICOLON;
    t[0x28] = KEY_APOSTROPHE;
    t[0x29] = KEY_GRAVE;
    t[0x2A] = KEY_LEFTSHIFT;
    t[0x2B] = KEY_BACKSLASH;
    t[0x2C] = KEY_Z;
    t[0x2D] = KEY_X;
    t[0x2E] = KEY_C;
    t[0x2F] = KEY_V;
    t[0x30] = KEY_B;
    t[0x31] = KEY_N;
    t[0x32] = KEY_M;
    t[0x33] = KEY_COMMA;
    t[0x34] = KEY_DOT;
    t[0x35] = KEY_SLASH;
    t[0x36] = KEY_RIGHTSHIFT;
    t[0x37] = KEY_KPASTERISK;
    t[0x38] = KEY_LEFTALT;
    t[0x39] = KEY_SPACE;
    t[0x3A] = KEY_CAPSLOCK;
    t[0x3B] = KEY_F1;
    t[0x3C] = KEY_F2;
    t[0x3D] = KEY_F3;
    t[0x3E] = KEY_F4;
    t[0x3F] = KEY_F5;
    t[0x40] = KEY_F6;
    t[0x41] = KEY_F7;
    t[0x42] = KEY_F8;
    t[0x43] = KEY_F9;
    t[0x44] = KEY_F10;
    t[0x45] = KEY_NUMLOCK;
    t[0x46] = KEY_SCROLLLOCK;
    t[0x47] = KEY_KP7;
    t[0x48] = KEY_KP8;
    t[0x49] = KEY_KP9;
    t[0x4A] = KEY_KPMINUS;
    t[0x4B] = KEY_KP4;
    t[0x4C] = KEY_KP5;
    t[0x4D] = KEY_KP6;
    t[0x4E] = KEY_KPPLUS;
    t[0x4F] = KEY_KP1;
    t[0x50] = KEY_KP2;
    t[0x51] = KEY_KP3;
    t[0x52] = KEY_KP0;
    t[0x53] = KEY_KPDOT;
    t[0x57] = KEY_F11;
    t[0x58] = KEY_F12;
    t
};

static E0_SCANCODE_TABLE: [u16; 128] = {
    let mut t = [0u16; 128];
    t[0x1C] = KEY_KPENTER;
    t[0x1D] = KEY_RIGHTCTRL;
    t[0x35] = KEY_KPSLASH;
    t[0x37] = KEY_SYSRQ;
    t[0x38] = KEY_RIGHTALT;
    t[0x47] = KEY_HOME;
    t[0x48] = KEY_UP;
    t[0x49] = KEY_PAGEUP;
    t[0x4B] = KEY_LEFT;
    t[0x4D] = KEY_RIGHT;
    t[0x4F] = KEY_END;
    t[0x50] = KEY_DOWN;
    t[0x51] = KEY_PAGEDOWN;
    t[0x52] = KEY_INSERT;
    t[0x53] = KEY_DELETE;
    t[0x5B] = KEY_LEFTMETA;
    t[0x5C] = KEY_RIGHTMETA;
    t
};

struct Ps2Keyboard {
    /// Bitmap of supported keys (KEY_CNT / 8 bytes).
    key_bits: [u8; (KEY_CNT as usize).div_ceil(8)],
}

impl Ps2Keyboard {
    fn new() -> Self {
        let mut key_bits = [0u8; (KEY_CNT as usize).div_ceil(8)];

        // Mark all keys in both scancode tables as supported.
        for &code in SCANCODE_TABLE.iter().chain(E0_SCANCODE_TABLE.iter()) {
            if code != 0 {
                let byte = (code / 8) as usize;
                let bit = code % 8;
                if byte < key_bits.len() {
                    key_bits[byte] |= 1 << bit;
                }
            }
        }

        Self { key_bits }
    }
}

impl EventDeviceOps for Ps2Keyboard {
    fn name(&self) -> &str {
        "PS/2 Keyboard"
    }

    fn id(&self) -> InputId {
        InputId {
            bustype: BUS_I8042,
            vendor: 0x0001,
            product: 0x0001,
            version: 0x0001,
        }
    }

    fn supported_events(&self) -> u32 {
        (1 << EV_SYN) | (1 << EV_KEY) | (1 << EV_MSC)
    }

    fn supported_keys(&self) -> &[u8] {
        &self.key_bits
    }
}

struct KeyboardIrqHandler {
    device: Arc<EventDevice>,
    e0_prefix: bool,
}

impl IrqHandler for KeyboardIrqHandler {
    fn raise(&mut self) -> Status {
        let status = unsafe { read8(STATUS_PORT) };
        if status & STATUS_OUTPUT_FULL == 0 || status & STATUS_MOUSE_DATA != 0 {
            return Status::Ignored;
        }
        let scancode = unsafe { read8(DATA_PORT) };

        // E0 prefix: next byte is extended scancode.
        if scancode == 0xE0 {
            self.e0_prefix = true;
            return Status::Handled;
        }

        let release = scancode & 0x80 != 0;
        let index = (scancode & 0x7F) as usize;

        let keycode = if self.e0_prefix {
            self.e0_prefix = false;
            E0_SCANCODE_TABLE[index]
        } else {
            SCANCODE_TABLE[index]
        };

        if keycode != 0 {
            let value = if release { 0 } else { 1 };
            self.device.report_event(EV_KEY, keycode, value);
            self.device.report_event(EV_SYN, SYN_REPORT, 0);
        }

        Status::Handled
    }
}

struct Ps2Mouse {
    rel_bits: [u8; (REL_CNT as usize + 7) / 8],
    key_bits: [u8; (KEY_CNT as usize + 7) / 8],
}

impl Ps2Mouse {
    fn new() -> Self {
        let mut rel_bits = [0u8; (REL_CNT as usize + 7) / 8];
        let mut key_bits = [0u8; (KEY_CNT as usize + 7) / 8];

        // REL_X, REL_Y
        rel_bits[(REL_X / 8) as usize] |= 1 << (REL_X % 8);
        rel_bits[(REL_Y / 8) as usize] |= 1 << (REL_Y % 8);

        // BTN_LEFT, BTN_RIGHT, BTN_MIDDLE
        for btn in [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE] {
            let byte = (btn / 8) as usize;
            let bit = btn % 8;
            if byte < key_bits.len() {
                key_bits[byte] |= 1 << bit;
            }
        }

        Self { rel_bits, key_bits }
    }
}

impl EventDeviceOps for Ps2Mouse {
    fn name(&self) -> &str {
        "PS/2 Generic Mouse"
    }

    fn id(&self) -> InputId {
        InputId {
            bustype: BUS_I8042,
            vendor: 0x0002,
            product: 0x0001,
            version: 0x0001,
        }
    }

    fn supported_events(&self) -> u32 {
        (1 << EV_SYN) | (1 << EV_KEY) | (1 << EV_REL)
    }

    fn supported_keys(&self) -> &[u8] {
        &self.key_bits
    }

    fn supported_rel(&self) -> &[u8] {
        &self.rel_bits
    }
}

struct MouseIrqHandler {
    device: Arc<EventDevice>,
    packet: [u8; 3],
    byte_index: u8,
    prev_buttons: u8,
}

impl IrqHandler for MouseIrqHandler {
    fn raise(&mut self) -> Status {
        // IRQ 12 is exclusively the mouse line, so we don't need to check
        // STATUS_MOUSE_DATA — just verify data is available.
        if unsafe { read8(STATUS_PORT) } & STATUS_OUTPUT_FULL == 0 {
            return Status::Ignored;
        }
        let byte = unsafe { read8(DATA_PORT) };

        // First byte must have the alignment bit set for a valid PS/2 packet.
        if self.byte_index == 0 && byte & (1 << 3) == 0 {
            // Resync
            return Status::Handled;
        }

        self.packet[self.byte_index as usize] = byte;
        self.byte_index += 1;

        if self.byte_index < 3 {
            return Status::Handled;
        }

        // Complete 3-byte packet.
        self.byte_index = 0;

        let flags = self.packet[0];
        let buttons = flags & 0x07;

        // Sign-extend movement deltas.
        let mut dx = self.packet[1] as i32;
        let mut dy = self.packet[2] as i32;

        if flags & 0x10 != 0 {
            dx -= 256; // X sign bit
        }
        if flags & 0x20 != 0 {
            dy -= 256; // Y sign bit
        }

        // PS/2 Y axis is inverted, unlike evdev protocol.
        dy = -dy;

        let changed = buttons ^ self.prev_buttons;
        if changed & 0x01 != 0 {
            self.device
                .report_event(EV_KEY, BTN_LEFT, (buttons & 0x01) as i32);
        }
        if changed & 0x02 != 0 {
            self.device
                .report_event(EV_KEY, BTN_RIGHT, ((buttons >> 1) & 0x01) as i32);
        }
        if changed & 0x04 != 0 {
            self.device
                .report_event(EV_KEY, BTN_MIDDLE, ((buttons >> 2) & 0x01) as i32);
        }
        self.prev_buttons = buttons;

        // Report movement
        if dx != 0 {
            self.device.report_event(EV_REL, REL_X, dx);
        }
        if dy != 0 {
            self.device.report_event(EV_REL, REL_Y, dy);
        }

        // Sync
        if dx != 0 || dy != 0 || changed != 0 {
            self.device.report_event(EV_SYN, SYN_REPORT, 0);
        }

        Status::Handled
    }
}

#[initgraph::task(
    name = "arch.x86_64.ps2",
    depends = [
        crate::device::input::INPUT_STAGE,
        crate::vfs::VFS_STAGE,
        super::apic::IOAPIC_STAGE,
    ],
)]
fn PS2_STAGE() {
    if !BootInfo::get().command_line.get_bool("ps2").unwrap_or(true) {
        return;
    }

    Ps2Controller::init();

    let kbd_ops = Arc::new(Ps2Keyboard::new());
    let kbd_dev = EventDevice::new(kbd_ops);
    kbd_dev
        .register_device()
        .expect("Unable to register PS/2 keyboard");

    if let Some(irq) = super::apic::get_isa_irq(1) {
        let irq = irq as Arc<dyn IrqLine>;
        irq.attach(Box::new(KeyboardIrqHandler {
            device: kbd_dev,
            e0_prefix: false,
        }));
        irq.unmask();
    } else {
        log!("Keyboard IRQ 1 not available");
    }

    let mouse_ops = Arc::new(Ps2Mouse::new());
    let mouse_dev = EventDevice::new(mouse_ops);
    mouse_dev
        .register_device()
        .expect("Unable to register PS/2 mouse");

    if let Some(irq) = super::apic::get_isa_irq(12) {
        let irq = irq as Arc<dyn IrqLine>;
        irq.attach(Box::new(MouseIrqHandler {
            device: mouse_dev,
            packet: [0; 3],
            byte_index: 0,
            prev_buttons: 0,
        }));
        irq.unmask();
    } else {
        log!("Mouse IRQ 12 not available");
    }

    Ps2Controller::enable_irqs();

    log!("Keyboard and mouse initialized");
}
