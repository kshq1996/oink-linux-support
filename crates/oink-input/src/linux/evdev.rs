//! evdev 设备层：读 `/dev/input/event*`。
//!
//! Wayland 会话里 XRecord 看不到真正的输入：合成器把指针和键盘握在自己手里，
//! XWayland 只把事件转给 X11 客户端，Wayland 桌面上的指针移动根本不会进 X11。
//! 所以 Wayland 上直接读内核的输入设备。
//!
//! - 鼠标：相对位移（`REL_X`/`REL_Y`）。**合成器的加速曲线不在这条路上**：内核给的是
//!   原始计数，屏幕上的光标走的是加速之后的位移，所以本机追踪到的位置与光标会有偏差。
//! - 键盘：内核键码（`KEY_*`）。按键没有加速，映射是准的。
//! - 触摸板：合成器独占（内核设备报的是绝对坐标与手势），这里读不到它的移动。
//! - 自己注入的虚拟设备（见 [`super::uinput`]）必须跳过，否则会把自己写的位置
//!   当成用户移动报上去。

use std::fs::File;
use std::io::Read;
use std::os::fd::{AsRawFd, RawFd};

use super::scale;

/// 内核的输入设备目录。
const INPUT_DIR: &str = "/dev/input";

/// `struct input_event` 的大小（64 位下是 24 字节）。
pub(crate) const EVENT_SIZE: usize = 24;

// 内核的事件类型与代码（`include/uapi/linux/input-event-codes.h`）。
/// 事件类型：同步。
pub(crate) const EV_SYN: u16 = 0x00;
/// 事件类型：按键。
pub(crate) const EV_KEY: u16 = 0x01;
/// 事件类型：相对位移。
pub(crate) const EV_REL: u16 = 0x02;
/// 同步事件：一批事件的结束。
pub(crate) const SYN_REPORT: u16 = 0x00;
/// 相对位移：横轴。
pub(crate) const REL_X: u16 = 0x00;
/// 相对位移：纵轴。
pub(crate) const REL_Y: u16 = 0x01;
/// 相对位移：水平滚轮。
pub(crate) const REL_HWHEEL: u16 = 0x06;
/// 相对位移：垂直滚轮。
pub(crate) const REL_WHEEL: u16 = 0x08;
/// 按键：字母 A（用来认出键盘）。
pub(crate) const KEY_A: u16 = 30;
/// 鼠标左键。
pub(crate) const BTN_LEFT: u16 = 0x110;
/// 鼠标右键。
pub(crate) const BTN_RIGHT: u16 = 0x111;
/// 鼠标中键。
pub(crate) const BTN_MIDDLE: u16 = 0x112;
/// 鼠标侧键「后退」。
pub(crate) const BTN_SIDE: u16 = 0x113;
/// 鼠标侧键「前进」。
pub(crate) const BTN_EXTRA: u16 = 0x114;

/// 从内核读到的一个事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Raw {
    /// 事件类型（`EV_*`）。
    pub(crate) kind: u16,
    /// 事件代码（`REL_*`、`KEY_*`、`BTN_*`）。
    pub(crate) code: u16,
    /// 事件值。
    pub(crate) value: i32,
}

/// 一台输入设备。
pub(crate) struct Device {
    /// 设备文件。
    pub(crate) file: File,
    /// 设备名：用来跳过自己注入的虚拟设备。
    pub(crate) name: String,
    /// 这台设备会报相对位移（鼠标）。
    pub(crate) pointer: bool,
    /// 这台设备会报鼠标键。
    pub(crate) buttons: bool,
    /// 这台设备会报键盘按键。
    pub(crate) keyboard: bool,
    /// 这是手柄的指针节点：桌面（libinput）可能把它当鼠标用，
    /// 摇杆一动光标就跟着动。截获不用它，但远端接管指针时要独占它。
    pub(crate) gamepad_pointer: bool,
    /// 每逻辑像素对应多少设备计数。
    pub(crate) counts_per_pixel: f64,
}

impl Device {
    /// 读一批事件，返回解析出来的事件。
    ///
    /// 读不到东西时返回空批次（非阻塞读会这样）。
    pub(crate) fn read(&mut self, buffer: &mut [u8]) -> Vec<Raw> {
        let mut raw = Vec::new();
        loop {
            let read = match self.file.read(buffer) {
                Ok(read) => read,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            };
            if read == 0 {
                break;
            }
            let mut offset = 0;
            while offset + EVENT_SIZE <= read {
                raw.push(parse_event(&buffer[offset..offset + EVENT_SIZE]));
                offset += EVENT_SIZE;
            }
            if read < buffer.len() {
                // 内核一次给的是完整的一批事件，剩下的是下一批。
                break;
            }
        }
        raw
    }
}

/// 解析一个 `struct input_event`。
fn parse_event(bytes: &[u8]) -> Raw {
    // `struct input_event`：时间（`timeval`，两个 `long`）、类型、代码、值。
    let kind = u16::from_ne_bytes([bytes[16], bytes[17]]);
    let code = u16::from_ne_bytes([bytes[18], bytes[19]]);
    let value = i32::from_ne_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Raw { kind, code, value }
}

/// 打开所有能用的输入设备。
///
/// 打不开的设备跳过：普通用户没有权限读某些设备（那是给合成器准备的）。
pub(crate) fn open_devices() -> Vec<Device> {
    let Ok(entries) = std::fs::read_dir(INPUT_DIR) else {
        return Vec::new();
    };
    let mut devices = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.starts_with("event") {
            continue;
        }
        let Ok(file) = File::open(&path) else {
            continue;
        };
        let fd = file.as_raw_fd();
        let types = capability_bits(fd, 0, 8);
        if !bit_set(&types, EV_KEY) && !bit_set(&types, EV_REL) {
            continue;
        }
        let name = device_name(fd);
        let rel = capability_bits(fd, EV_REL as u8, 8);
        let reports_motion = bit_set(&types, EV_REL) && bit_set(&rel, REL_X) && bit_set(&rel, REL_Y);
        let keys = capability_bits(fd, EV_KEY as u8, 128);
        let roles = classify(&name, reports_motion, bit_set(&keys, BTN_LEFT), bit_set(&keys, KEY_A));
        devices.push(Device {
            file,
            name,
            pointer: roles.pointer,
            buttons: roles.buttons,
            keyboard: roles.keyboard,
            // 手柄的指针节点：截获不碰它，但要留着它的文件描述符，
            // 远端接管指针时用它独占设备（见 [`grab`]）。
            gamepad_pointer: roles.gamepad_pointer,
            // 设备计数换算成逻辑像素的系数（鼠标的 DPI 决定）。
            counts_per_pixel: scale::counts_per_pixel(&path),
        });
    }
    devices
}

/// 设备名看起来像手柄吗？
///
/// 内核不区分手柄节点和鼠标节点：同一个手柄会同时开出 `js0`（摇杆）、`event*`
/// （按键）和一个鼠标节点（摇杆当鼠标用）。udev 的 `ID_INPUT_JOYSTICK` 帮不上忙，
/// 手柄的鼠标节点在 udev 里就标着 `ID_INPUT_MOUSE=1`，只能按名字排除。
fn is_gamepad(name: &str) -> bool {
    let name = name.to_lowercase();
    [
        "controller",
        "gamepad",
        "joystick",
        "xbox",
        "playstation",
        "dualsense",
        "dualshock",
        "8bitdo",
        "nacon",
        // SteamOS 的 InputPlumber 把手柄的摇杆做成一个虚拟鼠标，
        // 它在 udev 里就是一个普通鼠标（`ID_INPUT_MOUSE=1`）。
        "inputplumber",
    ]
    .iter()
    .any(|marker| name.contains(marker))
}
/// 一台设备在截获里扮演的角色。
struct Roles {
    /// 指针（相对位移）。
    pointer: bool,
    /// 鼠标键。
    buttons: bool,
    /// 键盘按键。
    keyboard: bool,
    /// 手柄的指针节点：桌面可能把它当鼠标用，截获不用它，但要能独占它。
    gamepad_pointer: bool,
}

/// 一台设备在截获里扮演什么角色。
///
/// 手柄的节点不参与截获：内核把摇杆开成鼠标节点（`REL_X`/`REL_Y`），
/// 把按键开成键盘节点（`KEY_A`）。不排除的话，摇杆一动共享指针就跳，
/// 手柄的按键还会变成发给对端的按键。
fn classify(name: &str, reports_motion: bool, left_button: bool, key_a: bool) -> Roles {
    let gamepad = is_gamepad(name);
    Roles {
        pointer: !gamepad && reports_motion,
        buttons: !gamepad && left_button,
        keyboard: !gamepad && key_a,
        gamepad_pointer: gamepad && reports_motion,
    }
}

/// 独占一台输入设备：别的读者（合成器）再也收不到它的事件。
///
/// `EVIOCGRAB` 是设备级的：内核只把事件送给抓着它的这个描述符。
/// 参数是 `int`：1 独占，0 放开。
///
/// 手柄的指针节点被桌面当成鼠标时，摇杆会和注入的指针拉扯，
/// 光标像是被摇杆拖回原处。远端接管指针期间要独占这些节点。
pub(crate) fn grab(device: &Device, on: bool) -> bool {
    let value: libc::c_int = i32::from(on);
    let request = ioc(IOC_WRITE, EVDEV, 0x90, std::mem::size_of::<libc::c_int>() as u32);
    // SAFETY: `device` holds an open evdev file descriptor; the ioctl reads an
    // `int` from the pointer we pass, and that int outlives the call.
    let result = unsafe {
        libc::ioctl(
            device.file.as_raw_fd(),
            request,
            &value as *const libc::c_int as *mut libc::c_void,
        )
    };
    result >= 0
}

/// 读设备的能力位图。
fn capability_bits(fd: RawFd, ev: u8, len: usize) -> Vec<u8> {
    let mut bits = vec![0u8; len];
    let request = ioc(IOC_READ, EVDEV, 0x20 + ev, len as u32);
    // SAFETY: `fd` is an open evdev device; the kernel writes at most `len` bytes
    // into the buffer, which is exactly `len` long.
    let result = unsafe { libc::ioctl(fd, request, bits.as_mut_ptr()) };
    if result < 0 {
        bits.fill(0);
    }
    bits
}

/// 位图里第 `code` 位是不是 1。
fn bit_set(bits: &[u8], code: u16) -> bool {
    let index = usize::from(code) / 8;
    let bit = code % 8;
    bits.get(index).is_some_and(|byte| byte & (1 << bit) != 0)
}

/// 读设备名。
fn device_name(fd: RawFd) -> String {
    let mut buffer = [0u8; 256];
    let request = ioc(IOC_READ, EVDEV, 0x06, buffer.len() as u32);
    // SAFETY: `fd` is an open evdev device; the kernel writes at most
    // `buffer.len()` bytes (the name plus its terminator) into the buffer.
    let result = unsafe { libc::ioctl(fd, request, buffer.as_mut_ptr()) };
    if result < 0 {
        return String::new();
    }
    let end = buffer.iter().position(|byte| *byte == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

const fn ioc(direction: u32, kind: u8, number: u8, size: u32) -> libc::c_ulong {
    ((direction << 30) | (size << 16) | ((kind as u32) << 8) | (number as u32)) as libc::c_ulong
}

/// `IOC_READ`。
const IOC_READ: u32 = 2;
/// `IOC_WRITE`。
const IOC_WRITE: u32 = 1;
/// `ioctl` 的类型：输入设备。
const EVDEV: u8 = b'E';
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capability_bitmap_is_read_as_bits() {
        let bits = [0b0000_0101u8, 0b1000_0000];
        assert!(bit_set(&bits, 0));
        assert!(!bit_set(&bits, 1));
        assert!(bit_set(&bits, 2));
        assert!(!bit_set(&bits, 7));
        assert!(bit_set(&bits, 15));
        assert!(!bit_set(&bits, 8));
        assert!(!bit_set(&bits, 64));
        assert!(!bit_set(&bits, 200));
    }

    #[test]
    fn an_event_is_parsed_from_its_kernel_layout() {
        let mut bytes = [0u8; EVENT_SIZE];
        bytes[16..18].copy_from_slice(&EV_KEY.to_ne_bytes());
        bytes[18..20].copy_from_slice(&KEY_A.to_ne_bytes());
        bytes[20..24].copy_from_slice(&1i32.to_ne_bytes());
        let event = parse_event(&bytes);
        assert_eq!(
            event,
            Raw {
                kind: EV_KEY,
                code: KEY_A,
                value: 1
            }
        );
    }

    #[test]
    fn a_gamepad_is_not_mistaken_for_a_mouse() {
        // 手柄的鼠标节点：内核给它开 `REL_X`/`REL_Y` 和 `BTN_LEFT`。
        assert!(is_gamepad("Xbox360 Controller for Windows Mouse"));
        assert!(is_gamepad("Sony Interactive Entertainment Wireless Controller"));
        assert!(is_gamepad("8BitDo Ultimate Wireless"));
        // SteamOS 的 InputPlumber 把摇杆做成一个虚拟鼠标。
        assert!(is_gamepad("InputPlumber Mouse"));
        // 真鼠标和键盘不能被误伤。
        assert!(!is_gamepad("AT Translated Set 2 keyboard"));
        assert!(!is_gamepad(""));
    }

    #[test]
    fn a_gamepad_node_takes_no_role_in_the_capture() {
        // 手柄的鼠标节点：相对位移 + 鼠标左键，都不参与截获，
        // 但要标成手柄指针，桌面才拿不到它。
        let roles = classify("Xbox360 Controller for Windows Mouse", true, true, false);
        assert!(!roles.pointer);
        assert!(!roles.buttons);
        assert!(!roles.keyboard);
        assert!(roles.gamepad_pointer);

        // 手柄的键盘节点：内核给它开 `KEY_A`（手柄按键），
        // 那不能变成发给对端的键盘按键。
        let roles = classify("Xbox360 Controller for Windows Keyboard", false, false, true);
        assert!(!roles.pointer);
        assert!(!roles.buttons);
        assert!(!roles.keyboard);
        assert!(!roles.gamepad_pointer);

        // SteamOS 的虚拟鼠标（摇杆）。
        let roles = classify("InputPlumber Mouse", true, true, false);
        assert!(!roles.pointer);
        assert!(roles.gamepad_pointer);
    }

    #[test]
    fn a_real_mouse_and_keyboard_keep_their_roles() {
        let roles = classify("Logitech USB Mouse", true, true, false);
        assert!(roles.pointer);
        assert!(roles.buttons);
        assert!(!roles.keyboard);
        assert!(!roles.gamepad_pointer);

        let roles = classify("AT Translated Set 2 keyboard", false, false, true);
        assert!(roles.keyboard);
        assert!(!roles.pointer);
    }
}
