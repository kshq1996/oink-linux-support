//! HID Usage ↔ X11 keysym 映射。
//!
//! X11 用「键码（keycode）」与「符号（keysym）」描述键盘，Oink 用 HID Usage，两个方向的换算都经过 keysym：
//! - 采集：键码 →（当前键盘映射里该键码的**第一个** keysym，即未加 Shift 的那一组）→ HID Usage；
//! - 注入：HID Usage → keysym →（在当前键盘映射里找第一个 keysym 相同的键码）。
//!
//! `x11rb` 只生成协议结构，没有 keysym 常量，下面的数值取自 X11 的 `X11/keysymdef.h` 与
//! `X11/XF86keysym.h`（`XF86XK_*` 以及 Linux 的 `_EVDEVK()` 扩展）。
//!
//! 表里只收无歧义的键：
//! - HID 0x64（Non-US \ and |）在 X11 上与普通反斜杠共用 keysym `0x5c`，无法唯一对应，未收录；
//! - Keyboard 页的 0x7f/0x80/0x81（静音 / 音量）与 Consumer 页的 0xe2/0xe9/0xea 是同一批媒体键，
//!   X11 只有一套 XF86 音量 keysym，因此只收录 Consumer 页那几个；
//! - `KeyCode::NONE` 与 `KeyCode::MENU_MASK` 不是真实按键，永远不在表里。

use oink_foundation::input::{KeyCode, MouseButton, ScrollUnit};

/// `(keysym, HID Usage)`，按 keysym 升序。
#[rustfmt::skip]
const BY_KEYSYM: &[(u32, KeyCode)] = &[
    (0x0020, KeyCode::keyboard(0x2c)), // space
    (0x0027, KeyCode::keyboard(0x34)), // apostrophe
    (0x002c, KeyCode::keyboard(0x36)), // comma
    (0x002d, KeyCode::keyboard(0x2d)), // minus
    (0x002e, KeyCode::keyboard(0x37)), // period
    (0x002f, KeyCode::keyboard(0x38)), // slash
    (0x0030, KeyCode::keyboard(0x27)), // 0
    (0x0031, KeyCode::keyboard(0x1e)), // 1
    (0x0032, KeyCode::keyboard(0x1f)), // 2
    (0x0033, KeyCode::keyboard(0x20)), // 3
    (0x0034, KeyCode::keyboard(0x21)), // 4
    (0x0035, KeyCode::keyboard(0x22)), // 5
    (0x0036, KeyCode::keyboard(0x23)), // 6
    (0x0037, KeyCode::keyboard(0x24)), // 7
    (0x0038, KeyCode::keyboard(0x25)), // 8
    (0x0039, KeyCode::keyboard(0x26)), // 9
    (0x003b, KeyCode::keyboard(0x33)), // semicolon
    (0x003d, KeyCode::keyboard(0x2e)), // equal
    (0x005b, KeyCode::keyboard(0x2f)), // bracketleft
    (0x005c, KeyCode::keyboard(0x31)), // backslash
    (0x005d, KeyCode::keyboard(0x30)), // bracketright
    (0x0060, KeyCode::keyboard(0x35)), // grave
    (0x0061, KeyCode::keyboard(0x04)), // a
    (0x0062, KeyCode::keyboard(0x05)), // b
    (0x0063, KeyCode::keyboard(0x06)), // c
    (0x0064, KeyCode::keyboard(0x07)), // d
    (0x0065, KeyCode::keyboard(0x08)), // e
    (0x0066, KeyCode::keyboard(0x09)), // f
    (0x0067, KeyCode::keyboard(0x0a)), // g
    (0x0068, KeyCode::keyboard(0x0b)), // h
    (0x0069, KeyCode::keyboard(0x0c)), // i
    (0x006a, KeyCode::keyboard(0x0d)), // j
    (0x006b, KeyCode::keyboard(0x0e)), // k
    (0x006c, KeyCode::keyboard(0x0f)), // l
    (0x006d, KeyCode::keyboard(0x10)), // m
    (0x006e, KeyCode::keyboard(0x11)), // n
    (0x006f, KeyCode::keyboard(0x12)), // o
    (0x0070, KeyCode::keyboard(0x13)), // p
    (0x0071, KeyCode::keyboard(0x14)), // q
    (0x0072, KeyCode::keyboard(0x15)), // r
    (0x0073, KeyCode::keyboard(0x16)), // s
    (0x0074, KeyCode::keyboard(0x17)), // t
    (0x0075, KeyCode::keyboard(0x18)), // u
    (0x0076, KeyCode::keyboard(0x19)), // v
    (0x0077, KeyCode::keyboard(0x1a)), // w
    (0x0078, KeyCode::keyboard(0x1b)), // x
    (0x0079, KeyCode::keyboard(0x1c)), // y
    (0x007a, KeyCode::keyboard(0x1d)), // z
    (0xff08, KeyCode::keyboard(0x2a)), // BackSpace
    (0xff09, KeyCode::keyboard(0x2b)), // Tab
    (0xff0d, KeyCode::keyboard(0x28)), // Return
    (0xff13, KeyCode::keyboard(0x48)), // Pause
    (0xff14, KeyCode::keyboard(0x47)), // Scroll_Lock
    (0xff1b, KeyCode::keyboard(0x29)), // Escape
    (0xff50, KeyCode::keyboard(0x4a)), // Home
    (0xff51, KeyCode::keyboard(0x50)), // Left
    (0xff52, KeyCode::keyboard(0x52)), // Up
    (0xff53, KeyCode::keyboard(0x4f)), // Right
    (0xff54, KeyCode::keyboard(0x51)), // Down
    (0xff55, KeyCode::keyboard(0x4b)), // Page_Up
    (0xff56, KeyCode::keyboard(0x4e)), // Page_Down
    (0xff57, KeyCode::keyboard(0x4d)), // End
    (0xff61, KeyCode::keyboard(0x46)), // Print
    (0xff63, KeyCode::keyboard(0x49)), // Insert
    (0xff67, KeyCode::keyboard(0x65)), // Menu
    (0xff7f, KeyCode::keyboard(0x53)), // Num_Lock
    (0xff8d, KeyCode::keyboard(0x58)), // KP_Enter
    (0xffaa, KeyCode::keyboard(0x55)), // KP_Multiply
    (0xffab, KeyCode::keyboard(0x57)), // KP_Add
    (0xffad, KeyCode::keyboard(0x56)), // KP_Subtract
    (0xffae, KeyCode::keyboard(0x63)), // KP_Decimal
    (0xffaf, KeyCode::keyboard(0x54)), // KP_Divide
    (0xffb0, KeyCode::keyboard(0x62)), // KP_0
    (0xffb1, KeyCode::keyboard(0x59)), // KP_1
    (0xffb2, KeyCode::keyboard(0x5a)), // KP_2
    (0xffb3, KeyCode::keyboard(0x5b)), // KP_3
    (0xffb4, KeyCode::keyboard(0x5c)), // KP_4
    (0xffb5, KeyCode::keyboard(0x5d)), // KP_5
    (0xffb6, KeyCode::keyboard(0x5e)), // KP_6
    (0xffb7, KeyCode::keyboard(0x5f)), // KP_7
    (0xffb8, KeyCode::keyboard(0x60)), // KP_8
    (0xffb9, KeyCode::keyboard(0x61)), // KP_9
    (0xffbd, KeyCode::keyboard(0x67)), // KP_Equal
    (0xffbe, KeyCode::keyboard(0x3a)), // F1
    (0xffbf, KeyCode::keyboard(0x3b)), // F2
    (0xffc0, KeyCode::keyboard(0x3c)), // F3
    (0xffc1, KeyCode::keyboard(0x3d)), // F4
    (0xffc2, KeyCode::keyboard(0x3e)), // F5
    (0xffc3, KeyCode::keyboard(0x3f)), // F6
    (0xffc4, KeyCode::keyboard(0x40)), // F7
    (0xffc5, KeyCode::keyboard(0x41)), // F8
    (0xffc6, KeyCode::keyboard(0x42)), // F9
    (0xffc7, KeyCode::keyboard(0x43)), // F10
    (0xffc8, KeyCode::keyboard(0x44)), // F11
    (0xffc9, KeyCode::keyboard(0x45)), // F12
    (0xffca, KeyCode::keyboard(0x68)), // F13
    (0xffcb, KeyCode::keyboard(0x69)), // F14
    (0xffcc, KeyCode::keyboard(0x6a)), // F15
    (0xffcd, KeyCode::keyboard(0x6b)), // F16
    (0xffce, KeyCode::keyboard(0x6c)), // F17
    (0xffcf, KeyCode::keyboard(0x6d)), // F18
    (0xffd0, KeyCode::keyboard(0x6e)), // F19
    (0xffd1, KeyCode::keyboard(0x6f)), // F20
    (0xffd2, KeyCode::keyboard(0x70)), // F21
    (0xffd3, KeyCode::keyboard(0x71)), // F22
    (0xffd4, KeyCode::keyboard(0x72)), // F23
    (0xffd5, KeyCode::keyboard(0x73)), // F24
    (0xffe1, KeyCode::keyboard(0xe1)), // Shift_L
    (0xffe2, KeyCode::keyboard(0xe5)), // Shift_R
    (0xffe3, KeyCode::keyboard(0xe0)), // Control_L
    (0xffe4, KeyCode::keyboard(0xe4)), // Control_R
    (0xffe5, KeyCode::keyboard(0x39)), // Caps_Lock
    (0xffe9, KeyCode::keyboard(0xe2)), // Alt_L
    (0xffea, KeyCode::keyboard(0xe6)), // Alt_R
    (0xffeb, KeyCode::keyboard(0xe3)), // Super_L
    (0xffec, KeyCode::keyboard(0xe7)), // Super_R
    (0xffff, KeyCode::keyboard(0x4c)), // Delete
    (0x1008ff02, KeyCode::new(KeyCode::PAGE_CONSUMER, 0x6f)), // XF86XK_MonBrightnessUp
    (0x1008ff03, KeyCode::new(KeyCode::PAGE_CONSUMER, 0x70)), // XF86XK_MonBrightnessDown
    (0x1008ff04, KeyCode::new(KeyCode::PAGE_CONSUMER, 0x7c)), // XF86XK_KbdLightOnOff
    (0x1008ff10, KeyCode::keyboard(0x66)), // XF86XK_Standby
    (0x1008ff11, KeyCode::new(KeyCode::PAGE_CONSUMER, 0xea)), // XF86XK_AudioLowerVolume
    (0x1008ff12, KeyCode::new(KeyCode::PAGE_CONSUMER, 0xe2)), // XF86XK_AudioMute
    (0x1008ff13, KeyCode::new(KeyCode::PAGE_CONSUMER, 0xe9)), // XF86XK_AudioRaiseVolume
    (0x1008ff14, KeyCode::new(KeyCode::PAGE_CONSUMER, 0xcd)), // XF86XK_AudioPlay
    (0x1008ff15, KeyCode::new(KeyCode::PAGE_CONSUMER, 0xb7)), // XF86XK_AudioStop
    (0x1008ff16, KeyCode::new(KeyCode::PAGE_CONSUMER, 0xb6)), // XF86XK_AudioPrev
    (0x1008ff17, KeyCode::new(KeyCode::PAGE_CONSUMER, 0xb5)), // XF86XK_AudioNext
    (0x1008ff3e, KeyCode::new(KeyCode::PAGE_CONSUMER, 0xb4)), // XF86XK_AudioRewind
    (0x1008ff57, KeyCode::keyboard(0x7c)), // XF86XK_Copy
    (0x1008ff58, KeyCode::keyboard(0x7b)), // XF86XK_Cut
    (0x1008ff6d, KeyCode::keyboard(0x7d)), // XF86XK_Paste
    (0x1008ff97, KeyCode::new(KeyCode::PAGE_CONSUMER, 0xb3)), // XF86XK_AudioForward
    (0x1008ffe5, KeyCode::new(KeyCode::PAGE_CONSUMER, 0x7a)), // XF86 KbdBrightnessDown
    (0x1008ffe6, KeyCode::new(KeyCode::PAGE_CONSUMER, 0x79)), // XF86 KbdBrightnessUp
];

/// `(HID Usage, keysym)`，按 HID Usage 升序；与 [`BY_KEYSYM`] 互为逆映射。
#[rustfmt::skip]
const BY_HID: &[(KeyCode, u32)] = &[
    (KeyCode::keyboard(0x04), 0x0061), // a
    (KeyCode::keyboard(0x05), 0x0062), // b
    (KeyCode::keyboard(0x06), 0x0063), // c
    (KeyCode::keyboard(0x07), 0x0064), // d
    (KeyCode::keyboard(0x08), 0x0065), // e
    (KeyCode::keyboard(0x09), 0x0066), // f
    (KeyCode::keyboard(0x0a), 0x0067), // g
    (KeyCode::keyboard(0x0b), 0x0068), // h
    (KeyCode::keyboard(0x0c), 0x0069), // i
    (KeyCode::keyboard(0x0d), 0x006a), // j
    (KeyCode::keyboard(0x0e), 0x006b), // k
    (KeyCode::keyboard(0x0f), 0x006c), // l
    (KeyCode::keyboard(0x10), 0x006d), // m
    (KeyCode::keyboard(0x11), 0x006e), // n
    (KeyCode::keyboard(0x12), 0x006f), // o
    (KeyCode::keyboard(0x13), 0x0070), // p
    (KeyCode::keyboard(0x14), 0x0071), // q
    (KeyCode::keyboard(0x15), 0x0072), // r
    (KeyCode::keyboard(0x16), 0x0073), // s
    (KeyCode::keyboard(0x17), 0x0074), // t
    (KeyCode::keyboard(0x18), 0x0075), // u
    (KeyCode::keyboard(0x19), 0x0076), // v
    (KeyCode::keyboard(0x1a), 0x0077), // w
    (KeyCode::keyboard(0x1b), 0x0078), // x
    (KeyCode::keyboard(0x1c), 0x0079), // y
    (KeyCode::keyboard(0x1d), 0x007a), // z
    (KeyCode::keyboard(0x1e), 0x0031), // 1
    (KeyCode::keyboard(0x1f), 0x0032), // 2
    (KeyCode::keyboard(0x20), 0x0033), // 3
    (KeyCode::keyboard(0x21), 0x0034), // 4
    (KeyCode::keyboard(0x22), 0x0035), // 5
    (KeyCode::keyboard(0x23), 0x0036), // 6
    (KeyCode::keyboard(0x24), 0x0037), // 7
    (KeyCode::keyboard(0x25), 0x0038), // 8
    (KeyCode::keyboard(0x26), 0x0039), // 9
    (KeyCode::keyboard(0x27), 0x0030), // 0
    (KeyCode::keyboard(0x28), 0xff0d), // Return
    (KeyCode::keyboard(0x29), 0xff1b), // Escape
    (KeyCode::keyboard(0x2a), 0xff08), // BackSpace
    (KeyCode::keyboard(0x2b), 0xff09), // Tab
    (KeyCode::keyboard(0x2c), 0x0020), // space
    (KeyCode::keyboard(0x2d), 0x002d), // minus
    (KeyCode::keyboard(0x2e), 0x003d), // equal
    (KeyCode::keyboard(0x2f), 0x005b), // bracketleft
    (KeyCode::keyboard(0x30), 0x005d), // bracketright
    (KeyCode::keyboard(0x31), 0x005c), // backslash
    (KeyCode::keyboard(0x33), 0x003b), // semicolon
    (KeyCode::keyboard(0x34), 0x0027), // apostrophe
    (KeyCode::keyboard(0x35), 0x0060), // grave
    (KeyCode::keyboard(0x36), 0x002c), // comma
    (KeyCode::keyboard(0x37), 0x002e), // period
    (KeyCode::keyboard(0x38), 0x002f), // slash
    (KeyCode::keyboard(0x39), 0xffe5), // Caps_Lock
    (KeyCode::keyboard(0x3a), 0xffbe), // F1
    (KeyCode::keyboard(0x3b), 0xffbf), // F2
    (KeyCode::keyboard(0x3c), 0xffc0), // F3
    (KeyCode::keyboard(0x3d), 0xffc1), // F4
    (KeyCode::keyboard(0x3e), 0xffc2), // F5
    (KeyCode::keyboard(0x3f), 0xffc3), // F6
    (KeyCode::keyboard(0x40), 0xffc4), // F7
    (KeyCode::keyboard(0x41), 0xffc5), // F8
    (KeyCode::keyboard(0x42), 0xffc6), // F9
    (KeyCode::keyboard(0x43), 0xffc7), // F10
    (KeyCode::keyboard(0x44), 0xffc8), // F11
    (KeyCode::keyboard(0x45), 0xffc9), // F12
    (KeyCode::keyboard(0x46), 0xff61), // Print
    (KeyCode::keyboard(0x47), 0xff14), // Scroll_Lock
    (KeyCode::keyboard(0x48), 0xff13), // Pause
    (KeyCode::keyboard(0x49), 0xff63), // Insert
    (KeyCode::keyboard(0x4a), 0xff50), // Home
    (KeyCode::keyboard(0x4b), 0xff55), // Page_Up
    (KeyCode::keyboard(0x4c), 0xffff), // Delete
    (KeyCode::keyboard(0x4d), 0xff57), // End
    (KeyCode::keyboard(0x4e), 0xff56), // Page_Down
    (KeyCode::keyboard(0x4f), 0xff53), // Right
    (KeyCode::keyboard(0x50), 0xff51), // Left
    (KeyCode::keyboard(0x51), 0xff54), // Down
    (KeyCode::keyboard(0x52), 0xff52), // Up
    (KeyCode::keyboard(0x53), 0xff7f), // Num_Lock
    (KeyCode::keyboard(0x54), 0xffaf), // KP_Divide
    (KeyCode::keyboard(0x55), 0xffaa), // KP_Multiply
    (KeyCode::keyboard(0x56), 0xffad), // KP_Subtract
    (KeyCode::keyboard(0x57), 0xffab), // KP_Add
    (KeyCode::keyboard(0x58), 0xff8d), // KP_Enter
    (KeyCode::keyboard(0x59), 0xffb1), // KP_1
    (KeyCode::keyboard(0x5a), 0xffb2), // KP_2
    (KeyCode::keyboard(0x5b), 0xffb3), // KP_3
    (KeyCode::keyboard(0x5c), 0xffb4), // KP_4
    (KeyCode::keyboard(0x5d), 0xffb5), // KP_5
    (KeyCode::keyboard(0x5e), 0xffb6), // KP_6
    (KeyCode::keyboard(0x5f), 0xffb7), // KP_7
    (KeyCode::keyboard(0x60), 0xffb8), // KP_8
    (KeyCode::keyboard(0x61), 0xffb9), // KP_9
    (KeyCode::keyboard(0x62), 0xffb0), // KP_0
    (KeyCode::keyboard(0x63), 0xffae), // KP_Decimal
    (KeyCode::keyboard(0x65), 0xff67), // Menu
    (KeyCode::keyboard(0x66), 0x1008ff10), // XF86XK_Standby
    (KeyCode::keyboard(0x67), 0xffbd), // KP_Equal
    (KeyCode::keyboard(0x68), 0xffca), // F13
    (KeyCode::keyboard(0x69), 0xffcb), // F14
    (KeyCode::keyboard(0x6a), 0xffcc), // F15
    (KeyCode::keyboard(0x6b), 0xffcd), // F16
    (KeyCode::keyboard(0x6c), 0xffce), // F17
    (KeyCode::keyboard(0x6d), 0xffcf), // F18
    (KeyCode::keyboard(0x6e), 0xffd0), // F19
    (KeyCode::keyboard(0x6f), 0xffd1), // F20
    (KeyCode::keyboard(0x70), 0xffd2), // F21
    (KeyCode::keyboard(0x71), 0xffd3), // F22
    (KeyCode::keyboard(0x72), 0xffd4), // F23
    (KeyCode::keyboard(0x73), 0xffd5), // F24
    (KeyCode::keyboard(0x7b), 0x1008ff58), // XF86XK_Cut
    (KeyCode::keyboard(0x7c), 0x1008ff57), // XF86XK_Copy
    (KeyCode::keyboard(0x7d), 0x1008ff6d), // XF86XK_Paste
    (KeyCode::keyboard(0xe0), 0xffe3), // Control_L
    (KeyCode::keyboard(0xe1), 0xffe1), // Shift_L
    (KeyCode::keyboard(0xe2), 0xffe9), // Alt_L
    (KeyCode::keyboard(0xe3), 0xffeb), // Super_L
    (KeyCode::keyboard(0xe4), 0xffe4), // Control_R
    (KeyCode::keyboard(0xe5), 0xffe2), // Shift_R
    (KeyCode::keyboard(0xe6), 0xffea), // Alt_R
    (KeyCode::keyboard(0xe7), 0xffec), // Super_R
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0x6f), 0x1008ff02), // XF86XK_MonBrightnessUp
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0x70), 0x1008ff03), // XF86XK_MonBrightnessDown
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0x79), 0x1008ffe6), // XF86 KbdBrightnessUp
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0x7a), 0x1008ffe5), // XF86 KbdBrightnessDown
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0x7c), 0x1008ff04), // XF86XK_KbdLightOnOff
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0xb3), 0x1008ff97), // XF86XK_AudioForward
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0xb4), 0x1008ff3e), // XF86XK_AudioRewind
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0xb5), 0x1008ff17), // XF86XK_AudioNext
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0xb6), 0x1008ff16), // XF86XK_AudioPrev
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0xb7), 0x1008ff15), // XF86XK_AudioStop
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0xcd), 0x1008ff14), // XF86XK_AudioPlay
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0xe2), 0x1008ff12), // XF86XK_AudioMute
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0xe9), 0x1008ff13), // XF86XK_AudioRaiseVolume
    (KeyCode::new(KeyCode::PAGE_CONSUMER, 0xea), 0x1008ff11), // XF86XK_AudioLowerVolume
];

/// keysym → HID Usage。表里没有的 keysym 返回 `None`。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn hid_from_keysym(keysym: u32) -> Option<KeyCode> {
    BY_KEYSYM
        .binary_search_by_key(&keysym, |&(k, _)| k)
        .ok()
        .map(|index| BY_KEYSYM[index].1)
}

/// HID Usage → keysym。表里没有的用法返回 `None`。
pub(crate) fn keysym_from_hid(code: KeyCode) -> Option<u32> {
    BY_HID
        .binary_search_by_key(&code.0, |&(hid, _)| hid.0)
        .ok()
        .map(|index| BY_HID[index].1)
}

/// 当前键盘映射：键码 ↔ keysym。
///
/// `keysyms` 按键码顺序排列，每个键码有 `per_keycode` 个 keysym，第一个是未加 Shift 的那一组。
/// 采集与注入都用第一个：物理键位不能因为 Shift 状态而变。
#[derive(Clone, Debug, Default)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct KeyboardMap {
    /// 映射里的最小键码。
    pub(crate) min_keycode: u8,
    /// 每个键码的 keysym 数。
    pub(crate) per_keycode: u8,
    /// 所有键码的 keysym（按键码顺序排列）。
    pub(crate) keysyms: Vec<u32>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl KeyboardMap {
    /// 映射是不是空的（还没读过键盘映射）。
    pub(crate) fn is_empty(&self) -> bool {
        self.per_keycode == 0 || self.keysyms.is_empty()
    }

    /// 键码 → 该键码的第一个 keysym。
    pub(crate) fn keysym(&self, keycode: u8) -> Option<u32> {
        if self.per_keycode == 0 || keycode < self.min_keycode {
            return None;
        }
        let offset = usize::from(keycode - self.min_keycode) * usize::from(self.per_keycode);
        let keysym = *self.keysyms.get(offset)?;
        (keysym != 0).then_some(keysym)
    }

    /// 第一个 keysym 等于 `keysym` 的键码。
    pub(crate) fn keycode(&self, keysym: u32) -> Option<u8> {
        let per = usize::from(self.per_keycode);
        if per == 0 {
            return None;
        }
        for (index, group) in self.keysyms.chunks(per).enumerate() {
            if group.first().copied() == Some(keysym) {
                let keycode = self.min_keycode.checked_add(u8::try_from(index).ok()?)?;
                return Some(keycode);
            }
        }
        None
    }
}

/// X11 的左键按钮号。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const BUTTON_LEFT: u8 = 1;
/// X11 的中键按钮号。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const BUTTON_MIDDLE: u8 = 2;
/// X11 的右键按钮号。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const BUTTON_RIGHT: u8 = 3;
/// X11 的「滚轮向上」按钮号。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const WHEEL_UP: u8 = 4;
/// X11 的「滚轮向下」按钮号。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const WHEEL_DOWN: u8 = 5;
/// X11 的「滚轮向左」按钮号。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const WHEEL_LEFT: u8 = 6;
/// X11 的「滚轮向右」按钮号。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const WHEEL_RIGHT: u8 = 7;
/// X11 的「后退」侧键按钮号。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const BUTTON_BACK: u8 = 8;
/// X11 的「前进」侧键按钮号。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const BUTTON_FORWARD: u8 = 9;

/// X11 按钮号 → [`MouseButton`]。
///
/// 侧键在 X11 上是 8 / 9；其余按钮按 0 起的编号记在 [`MouseButton::Other`] 里
/// （与 `MouseButton` 的约定一致：编号从 0 开始）。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn button_from_x11(button: u8) -> MouseButton {
    match button {
        BUTTON_LEFT => MouseButton::Left,
        BUTTON_MIDDLE => MouseButton::Middle,
        BUTTON_RIGHT => MouseButton::Right,
        BUTTON_BACK => MouseButton::Back,
        BUTTON_FORWARD => MouseButton::Forward,
        other => MouseButton::Other(other.saturating_sub(1)),
    }
}

/// [`MouseButton`] → X11 按钮号。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn x11_button(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => BUTTON_LEFT,
        MouseButton::Middle => BUTTON_MIDDLE,
        MouseButton::Right => BUTTON_RIGHT,
        MouseButton::Back => BUTTON_BACK,
        MouseButton::Forward => BUTTON_FORWARD,
        MouseButton::Other(n) => n.saturating_add(1).clamp(1, 255),
    }
}

/// 像素 → 滚轮格数的换算：与 Windows 端一致（`100/3` 像素一行，每格 3 行）。
const PIXELS_PER_LINE: f64 = 100.0 / 3.0;
/// X11 每格滚轮的行数，与 [`crate::FeelProfile::wheel_lines_per_notch`] 一致。
const LINES_PER_NOTCH: f64 = 3.0;
/// 单次注入最多发出的滚轮格数，避免异常大的滚动量刷爆事件队列。
const MAX_NOTCHES: u32 = 64;

/// 滚动量 → X11 滚轮按钮序列（每一格一个按钮，注入时按下后立刻抬起）。
///
/// 方向：`dy > 0`（视图向文档顶部移动，即传统滚轮向上）→ 按钮 4，`dy < 0` → 按钮 5；
/// `dx > 0` → 按钮 7，`dx < 0` → 按钮 6。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn scroll_buttons(dx: f64, dy: f64, unit: ScrollUnit) -> Vec<u8> {
    let (dx, dy) = scroll_notches(dx, dy, unit);
    let mut buttons = Vec::new();
    push_notches(&mut buttons, dy, WHEEL_UP, WHEEL_DOWN);
    push_notches(&mut buttons, dx, WHEEL_RIGHT, WHEEL_LEFT);
    buttons
}

/// 滚动量 → 格数（可带小数）。
///
/// 像素单位按 `100/3` 像素一行、每格 3 行换算；行单位直接当格数用。
pub(crate) fn scroll_notches(dx: f64, dy: f64, unit: ScrollUnit) -> (f64, f64) {
    match unit {
        ScrollUnit::Line => (dx, dy),
        ScrollUnit::Pixel => (
            dx / (PIXELS_PER_LINE * LINES_PER_NOTCH),
            dy / (PIXELS_PER_LINE * LINES_PER_NOTCH),
        ),
    }
}

/// 把一个方向的滚动量换算成若干个滚轮按钮。
fn push_notches(buttons: &mut Vec<u8>, value: f64, positive: u8, negative: u8) {
    if value == 0.0 {
        return;
    }
    // 小数滚轮（高精度滚轮）四舍五入；非零值至少发一格。
    let steps = value.abs().round().max(1.0).min(f64::from(MAX_NOTCHES)) as u32;
    let button = if value > 0.0 { positive } else { negative };
    buttons.extend(std::iter::repeat_n(button, steps as usize));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_keysym_round_trips() {
        for &(keysym, hid) in BY_KEYSYM {
            let back = hid_from_keysym(keysym).unwrap_or_else(|| panic!("keysym {keysym:#x} 没有 HID 用法"));
            assert_eq!(back, hid, "keysym {keysym:#x} 的 HID 用法不一致");
            assert_eq!(keysym_from_hid(hid), Some(keysym), "HID 用法 {hid:?} 的 keysym 不一致");
        }
    }

    #[test]
    fn every_hid_usage_round_trips() {
        for &(hid, keysym) in BY_HID {
            let back = keysym_from_hid(hid).unwrap_or_else(|| panic!("HID 用法 {hid:?} 没有 keysym"));
            assert_eq!(back, keysym, "HID 用法 {hid:?} 的 keysym 不一致");
            assert_eq!(
                hid_from_keysym(keysym),
                Some(hid),
                "keysym {keysym:#x} 的 HID 用法不一致"
            );
        }
    }

    #[test]
    fn tables_are_mutual_inverses() {
        assert_eq!(BY_KEYSYM.len(), BY_HID.len());
        for &(keysym, hid) in BY_KEYSYM {
            let entry = BY_HID
                .binary_search_by_key(&hid.0, |&(h, _)| h.0)
                .ok()
                .map(|index| BY_HID[index].1);
            assert_eq!(entry, Some(keysym));
        }
    }

    #[test]
    fn table_entries_are_unique() {
        let mut keysyms: Vec<u32> = BY_KEYSYM.iter().map(|&(k, _)| k).collect();
        keysyms.sort_unstable();
        let before = keysyms.len();
        keysyms.dedup();
        assert_eq!(before, keysyms.len(), "keysym 有重复");
        let mut hids: Vec<u32> = BY_HID.iter().map(|&(h, _)| h.0).collect();
        hids.sort_unstable();
        let before = hids.len();
        hids.dedup();
        assert_eq!(before, hids.len(), "HID 用法有重复");
    }

    #[test]
    fn representative_pairs_are_pinned() {
        assert_eq!(hid_from_keysym(0x0061), Some(KeyCode::keyboard(0x04))); // a
        assert_eq!(hid_from_keysym(0xff0d), Some(KeyCode::keyboard(0x28))); // Return
        assert_eq!(hid_from_keysym(0xff1b), Some(KeyCode::keyboard(0x29))); // Escape
        assert_eq!(hid_from_keysym(0xff51), Some(KeyCode::keyboard(0x50))); // Left
        assert_eq!(hid_from_keysym(0xffbe), Some(KeyCode::keyboard(0x3a))); // F1
        assert_eq!(keysym_from_hid(KeyCode::keyboard(0x04)), Some(0x0061));
        assert_eq!(keysym_from_hid(KeyCode::keyboard(0x28)), Some(0xff0d));
        assert_eq!(keysym_from_hid(KeyCode::keyboard(0x29)), Some(0xff1b));
        assert_eq!(keysym_from_hid(KeyCode::keyboard(0x50)), Some(0xff51));
        assert_eq!(keysym_from_hid(KeyCode::keyboard(0x3a)), Some(0xffbe));
    }

    #[test]
    fn modifiers_use_expected_keysyms() {
        assert_eq!(keysym_from_hid(KeyCode::keyboard(0xe0)), Some(0xffe3)); // Control_L
        assert_eq!(keysym_from_hid(KeyCode::keyboard(0xe1)), Some(0xffe1)); // Shift_L
        assert_eq!(keysym_from_hid(KeyCode::keyboard(0xe2)), Some(0xffe9)); // Alt_L
        assert_eq!(keysym_from_hid(KeyCode::keyboard(0xe3)), Some(0xffeb)); // Super_L
        assert_eq!(keysym_from_hid(KeyCode::keyboard(0xe4)), Some(0xffe4)); // Control_R
        assert_eq!(keysym_from_hid(KeyCode::keyboard(0xe5)), Some(0xffe2)); // Shift_R
        assert_eq!(keysym_from_hid(KeyCode::keyboard(0xe6)), Some(0xffea)); // Alt_R
        assert_eq!(keysym_from_hid(KeyCode::keyboard(0xe7)), Some(0xffec)); // Super_R
    }

    #[test]
    fn volume_and_brightness_use_consumer_page() {
        assert_eq!(
            keysym_from_hid(KeyCode::new(KeyCode::PAGE_CONSUMER, 0xe2)),
            Some(0x1008ff12)
        ); // AudioMute
        assert_eq!(
            keysym_from_hid(KeyCode::new(KeyCode::PAGE_CONSUMER, 0xe9)),
            Some(0x1008ff13)
        ); // AudioVolumeUp
        assert_eq!(
            keysym_from_hid(KeyCode::new(KeyCode::PAGE_CONSUMER, 0xea)),
            Some(0x1008ff11)
        ); // AudioVolumeDown
        assert_eq!(
            keysym_from_hid(KeyCode::new(KeyCode::PAGE_CONSUMER, 0x6f)),
            Some(0x1008ff02)
        ); // 亮度加
        assert_eq!(
            keysym_from_hid(KeyCode::new(KeyCode::PAGE_CONSUMER, 0x70)),
            Some(0x1008ff03)
        ); // 亮度减
    }

    #[test]
    fn none_and_menu_mask_are_not_physical_keys() {
        assert_eq!(keysym_from_hid(KeyCode::NONE), None);
        assert_eq!(keysym_from_hid(KeyCode::MENU_MASK), None);
        assert_eq!(hid_from_keysym(0), None);
    }

    #[test]
    fn buttons_map_to_x11_numbers() {
        assert_eq!(button_from_x11(1), MouseButton::Left);
        assert_eq!(button_from_x11(2), MouseButton::Middle);
        assert_eq!(button_from_x11(3), MouseButton::Right);
        assert_eq!(button_from_x11(8), MouseButton::Back);
        assert_eq!(button_from_x11(9), MouseButton::Forward);
        assert_eq!(button_from_x11(4), MouseButton::Other(3));
        assert_eq!(button_from_x11(5), MouseButton::Other(4));
        assert_eq!(x11_button(MouseButton::Left), 1);
        assert_eq!(x11_button(MouseButton::Middle), 2);
        assert_eq!(x11_button(MouseButton::Right), 3);
        assert_eq!(x11_button(MouseButton::Back), 8);
        assert_eq!(x11_button(MouseButton::Forward), 9);
        assert_eq!(x11_button(MouseButton::Other(3)), 4);
        for button in [1u8, 2, 3, 8, 9] {
            assert_eq!(x11_button(button_from_x11(button)), button);
        }
    }

    #[test]
    fn scroll_maps_to_wheel_buttons() {
        assert_eq!(scroll_buttons(0.0, 1.0, ScrollUnit::Line), vec![WHEEL_UP]);
        assert_eq!(scroll_buttons(0.0, -1.0, ScrollUnit::Line), vec![WHEEL_DOWN]);
        assert_eq!(scroll_buttons(1.0, 0.0, ScrollUnit::Line), vec![WHEEL_RIGHT]);
        assert_eq!(scroll_buttons(-1.0, 0.0, ScrollUnit::Line), vec![WHEEL_LEFT]);
        assert_eq!(scroll_buttons(0.0, 0.0, ScrollUnit::Line), Vec::<u8>::new());
        // 小数滚轮四舍五入，非零至少一格。
        assert_eq!(scroll_buttons(0.0, 0.4, ScrollUnit::Line), vec![WHEEL_UP]);
        assert_eq!(scroll_buttons(0.0, 2.4, ScrollUnit::Line), vec![WHEEL_UP, WHEEL_UP]);
        assert_eq!(
            scroll_buttons(0.0, -2.6, ScrollUnit::Line),
            vec![WHEEL_DOWN, WHEEL_DOWN, WHEEL_DOWN]
        );
        // 纵向在前、横向在后。
        assert_eq!(scroll_buttons(1.0, 1.0, ScrollUnit::Line), vec![WHEEL_UP, WHEEL_RIGHT]);
        // 像素单位按每格 100 像素换算。
        assert_eq!(scroll_buttons(0.0, 100.0, ScrollUnit::Pixel), vec![WHEEL_UP]);
        assert_eq!(scroll_buttons(0.0, 50.0, ScrollUnit::Pixel), vec![WHEEL_UP]);
        assert_eq!(scroll_buttons(0.0, 0.0, ScrollUnit::Pixel), Vec::<u8>::new());
    }

    #[test]
    fn keyboard_map_looks_up_keycodes() {
        let map = KeyboardMap {
            min_keycode: 8,
            per_keycode: 2,
            keysyms: vec![0xffe1, 0xffe1, 0x0061, 0x0041],
        };
        assert!(!map.is_empty());
        assert_eq!(map.keysym(8), Some(0xffe1));
        assert_eq!(map.keysym(9), Some(0x0061));
        assert_eq!(map.keysym(7), None);
        assert_eq!(map.keysym(10), None);
        assert_eq!(map.keycode(0xffe1), Some(8));
        assert_eq!(map.keycode(0x0061), Some(9));
        // 只看每组的第一个 keysym：加 Shift 后的同一个键不算另一个键位。
        assert_eq!(map.keycode(0x0041), None);
        assert!(KeyboardMap::default().is_empty());
        assert_eq!(KeyboardMap::default().keysym(8), None);
        assert_eq!(KeyboardMap::default().keycode(0x0061), None);
    }
}
