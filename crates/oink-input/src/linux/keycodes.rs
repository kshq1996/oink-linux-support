//! HID Usage ↔ Linux 内核键码。
//!
//! uinput 注入与 evdev 采集都用内核的键码（`KEY_*`），Oink 内部用 HID Usage，
//! 两边都要换算。键盘页（0x07）的用法与 PC 键盘的内核键码一一对应，键码取自
//! Linux 的 `include/uapi/linux/input-event-codes.h`。
//!
//! 表里只收 PC 键盘上无歧义的键：Non-US 键（0x64）与普通反斜杠在 X11 上共用
//! keysym，采集时无法区分，所以不收。

/// `(HID Usage, 内核键码)`，按 HID Usage 升序。
#[rustfmt::skip]
const BY_HID: &[(u16, u16)] = &[
    (0x04, 30),
    (0x05, 48),
    (0x06, 46),
    (0x07, 32),
    (0x08, 18),
    (0x09, 33),
    (0x0a, 34),
    (0x0b, 35),
    (0x0c, 23),
    (0x0d, 36),
    (0x0e, 37),
    (0x0f, 38),
    (0x10, 50),
    (0x11, 49),
    (0x12, 24),
    (0x13, 25),
    (0x14, 16),
    (0x15, 19),
    (0x16, 31),
    (0x17, 20),
    (0x18, 22),
    (0x19, 47),
    (0x1a, 17),
    (0x1b, 45),
    (0x1c, 21),
    (0x1d, 44),
    (0x1e, 2),
    (0x1f, 3),
    (0x20, 4),
    (0x21, 5),
    (0x22, 6),
    (0x23, 7),
    (0x24, 8),
    (0x25, 9),
    (0x26, 10),
    (0x27, 11),
    (0x28, 28),
    (0x29, 1),
    (0x2a, 14),
    (0x2b, 15),
    (0x2c, 57),
    (0x2d, 12),
    (0x2e, 13),
    (0x2f, 26),
    (0x30, 27),
    (0x31, 43),
    (0x33, 39),
    (0x34, 40),
    (0x35, 41),
    (0x36, 51),
    (0x37, 52),
    (0x38, 53),
    (0x39, 58),
    (0x3a, 59),
    (0x3b, 60),
    (0x3c, 61),
    (0x3d, 62),
    (0x3e, 63),
    (0x3f, 64),
    (0x40, 65),
    (0x41, 66),
    (0x42, 67),
    (0x43, 68),
    (0x44, 87),
    (0x45, 88),
    (0x46, 99),
    (0x47, 70),
    (0x48, 119),
    (0x49, 110),
    (0x4a, 102),
    (0x4b, 104),
    (0x4c, 111),
    (0x4d, 107),
    (0x4e, 109),
    (0x4f, 106),
    (0x50, 105),
    (0x51, 108),
    (0x52, 103),
    (0x53, 69),
    (0x55, 55),
    (0x56, 74),
    (0x57, 78),
    (0x58, 96),
    (0x59, 79),
    (0x5a, 80),
    (0x5b, 81),
    (0x5c, 82),
    (0x5d, 83),
    (0x5e, 84),
    (0x5f, 85),
    (0x60, 86),
    (0x61, 87),
    (0x62, 88),
    (0x63, 83),
    (0xe0, 29),
    (0xe1, 42),
    (0xe2, 56),
    (0xe3, 125),
    (0xe4, 97),
    (0xe5, 54),
    (0xe6, 100),
    (0xe7, 126),
];

/// 整张表：`(HID Usage, 内核键码)`，按 HID Usage 升序。
pub(crate) fn table() -> &'static [(u16, u16)] {
    BY_HID
}

/// HID Usage → 内核键码。
pub(crate) fn kernel(usage: u16) -> Option<u16> {
    BY_HID
        .binary_search_by_key(&usage, |(hid, _)| *hid)
        .ok()
        .map(|index| BY_HID[index].1)
}

/// 内核键码 → HID Usage。
pub(crate) fn hid(code: u16) -> Option<u16> {
    BY_HID.iter().find(|(_, kernel)| *kernel == code).map(|(hid, _)| *hid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_letters_map_both_ways() {
        assert_eq!(kernel(0x04), Some(30)); // a
        assert_eq!(kernel(0x1d), Some(44)); // z
        assert_eq!(hid(30), Some(0x04));
        assert_eq!(hid(44), Some(0x1d));
    }

    #[test]
    fn the_digits_map_both_ways() {
        assert_eq!(kernel(0x1e), Some(2)); // 1
        assert_eq!(kernel(0x27), Some(11)); // 0
        assert_eq!(hid(11), Some(0x27));
    }

    #[test]
    fn the_modifiers_map_both_ways() {
        assert_eq!(kernel(0xe1), Some(42)); // left shift
        assert_eq!(kernel(0xe7), Some(126)); // right meta
        assert_eq!(hid(125), Some(0xe3)); // left meta
    }

    #[test]
    fn the_table_has_no_duplicate_hid_usages() {
        let mut usages: Vec<u16> = BY_HID.iter().map(|(hid, _)| *hid).collect();
        usages.sort_unstable();
        let count = usages.len();
        usages.dedup();
        assert_eq!(usages.len(), count, "the table has a duplicate HID usage");
    }

    #[test]
    fn an_unknown_usage_has_no_keycode() {
        assert_eq!(kernel(0x00), None);
        assert_eq!(kernel(0x64), None); // Non-US backslash: ambiguous on X11.
        assert_eq!(hid(0), None);
    }
}
