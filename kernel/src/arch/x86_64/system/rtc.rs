use crate::{arch::x86_64::asm, clock};

const CMOS_ADDR: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

const REG_SECONDS: u8 = 0x00;
const REG_MINUTES: u8 = 0x02;
const REG_HOURS: u8 = 0x04;
const REG_DAY: u8 = 0x07;
const REG_MONTH: u8 = 0x08;
const REG_YEAR: u8 = 0x09;
const REG_STATUS_A: u8 = 0x0A;
const REG_STATUS_B: u8 = 0x0B;

const STATUS_A_UPDATE_IN_PROGRESS: u8 = 1 << 7;
const STATUS_B_24H: u8 = 1 << 1;
const STATUS_B_BINARY: u8 = 1 << 2;

unsafe fn read_register(reg: u8) -> u8 {
    unsafe {
        asm::write8(CMOS_ADDR, reg & 0x7F);
        asm::read8(CMOS_DATA)
    }
}

unsafe fn update_in_progress() -> bool {
    unsafe { read_register(REG_STATUS_A) & STATUS_A_UPDATE_IN_PROGRESS != 0 }
}

fn bcd_to_bin(value: u8) -> u8 {
    (value & 0x0F) + ((value >> 4) * 10)
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RawTime {
    seconds: u8,
    minutes: u8,
    hours: u8,
    day: u8,
    month: u8,
    year: u8,
}

unsafe fn read_raw() -> RawTime {
    unsafe {
        // Spin until the RTC is not in the middle of an update.
        while update_in_progress() {}
        RawTime {
            seconds: read_register(REG_SECONDS),
            minutes: read_register(REG_MINUTES),
            hours: read_register(REG_HOURS),
            day: read_register(REG_DAY),
            month: read_register(REG_MONTH),
            year: read_register(REG_YEAR),
        }
    }
}

fn read_unix_ns() -> Option<i64> {
    // Read twice and only accept the values when two consecutive reads agree;
    // this avoids races against the RTC's own update cycle.
    let mut last = unsafe { read_raw() };
    let mut current;
    loop {
        current = unsafe { read_raw() };
        if current == last {
            break;
        }
        last = current;
    }
    let status_b = unsafe { read_register(REG_STATUS_B) };

    let hours_raw = current.hours;
    let pm = (status_b & STATUS_B_24H) == 0 && (hours_raw & 0x80) != 0;

    let mut second = current.seconds;
    let mut minute = current.minutes;
    let mut hour = hours_raw & 0x7F;
    let mut day = current.day;
    let mut month = current.month;
    let mut year = current.year;

    if status_b & STATUS_B_BINARY == 0 {
        second = bcd_to_bin(second);
        minute = bcd_to_bin(minute);
        hour = bcd_to_bin(hour);
        day = bcd_to_bin(day);
        month = bcd_to_bin(month);
        year = bcd_to_bin(year);
    }

    if (status_b & STATUS_B_24H) == 0 {
        // 12-hour mode: 12 represents midnight/noon.
        hour %= 12;
        if pm {
            hour += 12;
        }
    }

    // The RTC year is two digits. Assume 2000+, which is good through 2099.
    let full_year = 2000u32 + year as u32;
    days_since_epoch(full_year, month as u32, day as u32).map(|days| {
        let secs = days * 86_400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64;
        secs.saturating_mul(1_000_000_000)
    })
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: u32, month: u32) -> Option<u32> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => return None,
    })
}

fn days_since_epoch(year: u32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) {
        return None;
    }
    let max_day = days_in_month(year, month)?;
    if day < 1 || day > max_day {
        return None;
    }
    if year < 1970 {
        return None;
    }

    let mut days: i64 = 0;
    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    for m in 1..month {
        days += days_in_month(year, m)? as i64;
    }
    days += (day - 1) as i64;
    Some(days)
}

#[initgraph::task(
    name = "arch.x86_64.rtc",
    depends = [crate::clock::CLOCK_STAGE],
)]
fn RTC_STAGE() {
    match read_unix_ns() {
        Some(ns) => {
            log!(
                "RTC: wall-clock time captured at {} seconds since the Unix epoch",
                ns / 1_000_000_000
            );
            clock::set_realtime_ns(ns);
        }
        None => {
            log!("RTC: failed to read a sensible time, CLOCK_REALTIME will return 0");
        }
    }
}
