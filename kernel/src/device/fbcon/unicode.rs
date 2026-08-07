use super::tables::{COMBINING, CP437, WIDE};

const fn bisearch(ucs: u32, table: &[(u32, u32)]) -> bool {
    if ucs < table[0].0 || ucs > table[table.len() - 1].1 {
        return false;
    }
    let (mut min, mut max) = (0isize, table.len() as isize - 1);
    while max >= min {
        let mid = (min + max) / 2;
        let (lo, hi) = table[mid as usize];
        if ucs > hi {
            min = mid + 1;
        } else if ucs < lo {
            max = mid - 1;
        } else {
            return true;
        }
    }
    false
}

pub(super) fn wcwidth(ucs: u32) -> u8 {
    if ucs < 32 || (0x7f..0xa0).contains(&ucs) {
        return 0;
    }
    if bisearch(ucs, &COMBINING) {
        return 0;
    }
    if bisearch(ucs, &WIDE) {
        return 2;
    }
    1
}

pub(super) fn cp437(code_point: u32) -> Option<u8> {
    if (0x2800..=0x28ff).contains(&code_point) {
        return Some(braille_to_cp437(code_point - 0x2800));
    }
    CP437
        .iter()
        .find(|&&(k, _)| k == code_point)
        .map(|&(_, v)| v)
}

const fn braille_to_cp437(dots: u32) -> u8 {
    if dots == 0 {
        return 0x20;
    }
    if dots == 0xff {
        return 0xdb;
    }
    let has_top = dots & 0x1b != 0;
    let has_bottom = dots & 0xe4 != 0;
    let has_left = dots & 0x47 != 0;
    let has_right = dots & 0xb8 != 0;
    if has_top && !has_bottom {
        return 0xdf;
    }
    if has_bottom && !has_top {
        return 0xdc;
    }
    if has_left && !has_right {
        return 0xdd;
    }
    if has_right && !has_left {
        return 0xde;
    }

    let mut n = dots - ((dots >> 1) & 0x55);
    n = (n & 0x33) + ((n >> 2) & 0x33);
    n = (n + (n >> 4)) & 0x0f;
    match n {
        0..=2 => 0xb0,
        3..=4 => 0xb1,
        5..=6 => 0xb2,
        _ => 0xdb,
    }
}
