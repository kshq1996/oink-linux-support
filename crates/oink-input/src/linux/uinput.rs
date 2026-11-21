//! uinput 注入：把 Oink 的事件写进内核的虚拟输入设备。
//!
//! Wayland 会话里 XTEST 没用：XWayland 的 X11 指针是假的，`XTestFakeMotionEvent`
//! 挪不动真正的光标（实测 `xdotool mousemove` 之后 X11 指针还在原处，屏幕上的
//! 光标也没动）。所以 Wayland 上改用 `/dev/uinput`：建一个虚拟的绝对定位设备，
//! 把光标放到哪个像素就写哪个绝对坐标。
//!
//! - 设备是**绝对**定位设备（`ABS_X`/`ABS_Y`）：相对移动会被合成器的加速曲线
//!   放大，绝对坐标不会，所以注入的位置和路由算出来的位置一致。
//! - `INPUT_PROP_DIRECT`：这是「直接指向」的设备（跟绘图板一类），合成器按设备
//!   的绝对范围一对一映射到屏幕，不当成触摸屏。
//! - 键盘与鼠标键用内核键码（`KEY_*`、`BTN_*`），见 [`super::keycodes`]。

use std::os::fd::{AsRawFd, RawFd};
use std::sync::Mutex;

use oink_foundation::Point;
use oink_foundation::input::{KeyCode, MouseButton, ScrollUnit};

use crate::InputError;
use crate::linux::keycodes;
use crate::linux::keymap;
use crate::linux::lock;

// 内核的事件类型与代码（`include/uapi/linux/input-event-codes.h`）。
const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;
const EV_ABS: u16 = 0x03;
const SYN_REPORT: u16 = 0x00;
const REL_HWHEEL: u16 = 0x06;
const REL_WHEEL: u16 = 0x08;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const BTN_SIDE: u16 = 0x113;
const BTN_EXTRA: u16 = 0x114;
const BTN_FORWARD: u16 = 0x115;
const BTN_BACK: u16 = 0x116;
const INPUT_PROP_DIRECT: i32 = 0x01;

/// `ioctl` 的请求号：`_IOC(dir, type, nr, size)`。
const fn ioc(direction: u32, kind: u8, number: u8, size: u32) -> libc::c_ulong {
    ((direction << 30) | (size << 16) | ((kind as u32) << 8) | (number as u32)) as libc::c_ulong
}

/// 滚轮的一个轴。
#[derive(Clone, Copy)]
enum ScrollAxis {
    Vertical,
    Horizontal,
}

/// 单次注入最多发出的滚轮格数，避免异常大的滚动量刷爆事件队列。
const MAX_WHEEL_NOTCHES: i64 = 64;

const IOC_NONE: u32 = 0;
const IOC_WRITE: u32 = 1;
const UINPUT: u8 = b'U';

const UI_SET_EVBIT: libc::c_ulong = ioc(IOC_WRITE, UINPUT, 100, 4);
const UI_SET_KEYBIT: libc::c_ulong = ioc(IOC_WRITE, UINPUT, 101, 4);
const UI_SET_RELBIT: libc::c_ulong = ioc(IOC_WRITE, UINPUT, 102, 4);
const UI_SET_ABSBIT: libc::c_ulong = ioc(IOC_WRITE, UINPUT, 103, 4);
const UI_SET_PROPBIT: libc::c_ulong = ioc(IOC_WRITE, UINPUT, 110, 4);
const UI_DEV_SETUP: libc::c_ulong = ioc(IOC_WRITE, UINPUT, 3, 92);
const UI_ABS_SETUP: libc::c_ulong = ioc(IOC_WRITE, UINPUT, 4, 28);
const UI_DEV_CREATE: libc::c_ulong = ioc(IOC_NONE, UINPUT, 1, 0);

/// `struct uinput_id`。
#[repr(C)]
struct UinputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

/// `struct uinput_setup`。
#[repr(C)]
struct UinputSetup {
    id: UinputId,
    name: [u8; 80],
    ff_effects_max: u32,
}

/// `struct uinput_abs_setup`。
///
/// 内核的头文件里 `filler` 是注释掉的，但 `input_absinfo` 要按 4 字节对齐，
/// 所以 `code` 后面有两个字节的填充，整个结构 28 字节。
#[repr(C)]
struct UinputAbsSetup {
    code: u16,
    padding: u16,
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

/// `struct input_event`。
#[repr(C)]
struct InputEvent {
    time: libc::timeval,
    kind: u16,
    code: u16,
    value: i32,
}

/// 虚拟输入设备：把 Oink 的事件写进内核。
pub(crate) struct Injector {
    fd: RawFd,
    /// 设备的绝对范围：桌面的像素尺寸。
    range: (u16, u16),
    /// 滚轮零头：竖直、水平各一份，单位是 1/120 格。
    scroll_remainder: Mutex<(f64, f64)>,
}

impl Injector {
    /// 建一个绝对定位的虚拟设备。
    ///
    /// `size` 是本机桌面的像素尺寸：设备的绝对范围就是它，合成器按这个范围
    /// 一对一映射到屏幕。
    pub(crate) fn open(name: &str, size: (u16, u16)) -> Result<Self, InputError> {
        // SAFETY: opening a device node with a NUL-terminated path; the flags are
        // the ordinary ones for a character device.
        let fd = unsafe {
            libc::open(
                c"/dev/uinput".as_ptr(),
                libc::O_WRONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(InputError::Platform(format!("打开 /dev/uinput 失败：{}", last_error())));
        }
        let injector = Self {
            fd,
            range: size,
            scroll_remainder: Mutex::new((0.0, 0.0)),
        };
        match injector.setup(name, size) {
            Ok(()) => Ok(injector),
            Err(error) => Err(error),
        }
    }

    /// 设备的绝对范围。
    pub(crate) fn range(&self) -> (u16, u16) {
        self.range
    }

    /// 告诉内核这个设备能发哪些事件。
    fn setup(&self, name: &str, size: (u16, u16)) -> Result<(), InputError> {
        self.set_bit(UI_SET_EVBIT, EV_KEY as i32)?;
        self.set_bit(UI_SET_EVBIT, EV_REL as i32)?;
        self.set_bit(UI_SET_EVBIT, EV_ABS as i32)?;
        // 鼠标键。
        for code in [
            BTN_LEFT,
            BTN_RIGHT,
            BTN_MIDDLE,
            BTN_SIDE,
            BTN_EXTRA,
            BTN_FORWARD,
            BTN_BACK,
        ] {
            self.set_bit(UI_SET_KEYBIT, code as i32)?;
        }
        // 键盘：表里有的键都发得出去。
        for (usage, _) in keycodes::table() {
            if let Some(code) = keycodes::kernel(*usage) {
                self.set_bit(UI_SET_KEYBIT, code as i32)?;
            }
        }
        // 滚轮。
        self.set_bit(UI_SET_RELBIT, REL_WHEEL as i32)?;
        self.set_bit(UI_SET_RELBIT, REL_HWHEEL as i32)?;
        // 绝对坐标轴：范围就是桌面尺寸。
        self.set_bit(UI_SET_ABSBIT, ABS_X as i32)?;
        self.set_bit(UI_SET_ABSBIT, ABS_Y as i32)?;
        self.absolute_setup(ABS_X, size.0)?;
        self.absolute_setup(ABS_Y, size.1)?;
        // 直接指向的设备：合成器按设备的绝对范围映射，不当成触摸屏。
        self.set_bit(UI_SET_PROPBIT, INPUT_PROP_DIRECT)?;
        // 设备信息。
        let mut setup = UinputSetup {
            id: UinputId {
                bustype: 0x03, // BUS_USB
                vendor: 0x1d6b,
                product: 0x0001,
                version: 1,
            },
            name: [0; 80],
            ff_effects_max: 0,
        };
        let bytes = name.as_bytes();
        let count = bytes.len().min(setup.name.len() - 1);
        setup.name[..count].copy_from_slice(&bytes[..count]);
        self.ioctl(UI_DEV_SETUP, &setup as *const UinputSetup as *mut libc::c_void)?;
        // 建好设备之后才能往里写事件。
        self.ioctl(UI_DEV_CREATE, std::ptr::null_mut())?;
        Ok(())
    }

    /// 把光标放到 `pos`（本机全局像素坐标）。
    pub(crate) fn move_to(&self, pos: Point, size: (u16, u16)) -> Result<(), InputError> {
        let x = clamp_axis(pos.x, size.0);
        let y = clamp_axis(pos.y, size.1);
        self.write_events(&[
            event(EV_ABS, ABS_X, x),
            event(EV_ABS, ABS_Y, y),
            event(EV_SYN, SYN_REPORT, 0),
        ])
    }

    /// 鼠标键。
    pub(crate) fn button(&self, button: MouseButton, pressed: bool) -> Result<(), InputError> {
        let code = match button {
            MouseButton::Left => BTN_LEFT,
            MouseButton::Right => BTN_RIGHT,
            MouseButton::Middle => BTN_MIDDLE,
            MouseButton::Back => BTN_BACK,
            MouseButton::Forward => BTN_FORWARD,
            MouseButton::Other(index) => 0x100 + u16::from(index),
        };
        self.write_events(&[event(EV_KEY, code, i32::from(pressed)), event(EV_SYN, SYN_REPORT, 0)])
    }

    /// 按键：HID Usage → 内核键码。
    pub(crate) fn key(&self, code: KeyCode, pressed: bool) -> Result<(), InputError> {
        if code.page() != KeyCode::PAGE_KEYBOARD {
            return Err(InputError::Platform(format!("这个键没有内核键码：{code:?}")));
        }
        let Some(kernel) = keycodes::kernel(code.usage()) else {
            return Err(InputError::Platform(format!("这个键没有内核键码：{code:?}")));
        };
        self.write_events(&[event(EV_KEY, kernel, i32::from(pressed)), event(EV_SYN, SYN_REPORT, 0)])
    }

    /// 滚动：一格滚轮是 `REL_WHEEL` 的一个单位。
    ///
    /// 符号约定与 X11 那边一致：`dy > 0` 是视图向文档顶部移动（传统滚轮向上滚），
    /// `dx > 0` 是视图向文档左侧移动。内核的 `REL_WHEEL` 正值就是「滚轮向上」，
    /// `REL_HWHEEL` 正值就是「滚轮向右」，与 X11 的按钮 4 / 按钮 6 同义。
    ///
    /// `REL_WHEEL` 是旧式滚轮：一个单位就是一格。高分辨率滚轮（1/120 格）
    /// 只有真实设备才有——内核靠滚轮轴的分辨率（120）额外发一份
    /// `REL_WHEEL_HI_RES`，而 uinput 设不了那个分辨率（`UI_DEV_SETUP` 只管
    /// 绝对轴），所以 uinput 设备在 libinput 那里永远是旧式滚轮：
    /// 按 1/120 格发会被当成 120 格，滚动快一百多倍。
    pub(crate) fn scroll(&self, dx: f64, dy: f64, unit: ScrollUnit) -> Result<(), InputError> {
        let (horizontal, vertical) = keymap::scroll_notches(dx, dy, unit);
        let mut events = Vec::new();
        // 不足一格的零头留到下次：四舍五入成整格会把触摸板的一小段滑动放大成
        // 一整格，滚动因此快好几倍（Windows 端就是按小数注入的）。
        let vertical_steps = self.scroll_units(ScrollAxis::Vertical, vertical);
        let horizontal_steps = self.scroll_units(ScrollAxis::Horizontal, horizontal);
        if vertical_steps != 0 {
            events.push(event(EV_REL, REL_WHEEL, vertical_steps));
        }
        if horizontal_steps != 0 {
            events.push(event(EV_REL, REL_HWHEEL, horizontal_steps));
        }
        if events.is_empty() {
            return Ok(());
        }
        events.push(event(EV_SYN, SYN_REPORT, 0));
        self.write_events(&events)
    }

    /// 滚轮量 → 内核的整格单位；不足一格的零头留到下次。
    fn scroll_units(&self, axis: ScrollAxis, notches: f64) -> i32 {
        let mut remainder = lock(&self.scroll_remainder);
        let slot = match axis {
            ScrollAxis::Vertical => &mut remainder.0,
            ScrollAxis::Horizontal => &mut remainder.1,
        };
        wheel_notches(slot, notches) as i32
    }

    /// 写一批事件。
    fn write_events(&self, events: &[InputEvent]) -> Result<(), InputError> {
        // SAFETY: `events` is a live slice of `InputEvent`; the byte view has the
        // same length and alignment requirements as the original data.
        let bytes = unsafe { std::slice::from_raw_parts(events.as_ptr() as *const u8, std::mem::size_of_val(events)) };
        let mut written = 0;
        while written < bytes.len() {
            // SAFETY: writing a plain byte slice to the device fd.
            let count = unsafe {
                libc::write(
                    self.fd,
                    bytes[written..].as_ptr() as *const libc::c_void,
                    bytes.len() - written,
                )
            };
            if count < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(InputError::Platform(format!("写虚拟输入设备失败：{error}")));
            }
            if count == 0 {
                return Err(InputError::Platform("写虚拟输入设备失败：设备不可写".into()));
            }
            written += count as usize;
        }
        Ok(())
    }

    /// 设置一个能力位。
    ///
    /// 内核的 `UI_SET_*BIT` 系列收的是**值本身**，不是指针：内核的实现是
    /// `uinput_set_bit(arg, evbit, EV_MAX)`，`arg` 就是那个编码（`/usr/include/linux/uinput.h`
    /// 里的 `_IOW(..., int)` 是老写法留下的假象）。传指针的话，内核把一个巨大的
    /// 地址当成编码，超过 `EV_MAX` 就回 EINVAL，虚拟设备根本建不起来。
    fn set_bit(&self, request: libc::c_ulong, bit: i32) -> Result<(), InputError> {
        self.ioctl_value(request, bit as libc::c_ulong)
    }

    /// 设置一个绝对坐标轴的范围。
    fn absolute_setup(&self, code: u16, extent: u16) -> Result<(), InputError> {
        let setup = UinputAbsSetup {
            code,
            padding: 0,
            value: 0,
            minimum: 0,
            maximum: i32::from(extent.saturating_sub(1)),
            fuzz: 0,
            flat: 0,
            resolution: 0,
        };
        self.ioctl(UI_ABS_SETUP, &setup as *const UinputAbsSetup as *mut libc::c_void)
    }

    /// 调一次 `ioctl`。
    fn ioctl(&self, request: libc::c_ulong, argument: *mut libc::c_void) -> Result<(), InputError> {
        // SAFETY: `request` is one of the uinput request numbers and `argument`
        // points at the matching structure; the kernel reads it as documented.
        let result = unsafe { libc::ioctl(self.fd, request, argument) };
        if result < 0 {
            return Err(InputError::Platform(format!("uinput ioctl 失败：{}", last_error())));
        }
        Ok(())
    }

    /// 调一次 `ioctl`，参数是**值**（内核直接从 `arg` 里读）。
    fn ioctl_value(&self, request: libc::c_ulong, argument: libc::c_ulong) -> Result<(), InputError> {
        // SAFETY: `request` is one of the uinput bit request numbers; the kernel
        // reads the argument as the code itself.
        let result = unsafe { libc::ioctl(self.fd, request, argument) };
        if result < 0 {
            return Err(InputError::Platform(format!("uinput ioctl 失败：{}", last_error())));
        }
        Ok(())
    }
}

/// 滚轮量 → 内核的整格单位（`REL_WHEEL` 一个单位就是一格）。
fn wheel_notches(remainder: &mut f64, notches: f64) -> i64 {
    keymap::take_notches(remainder, notches).clamp(-MAX_WHEEL_NOTCHES, MAX_WHEEL_NOTCHES)
}

impl AsRawFd for Injector {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl Drop for Injector {
    fn drop(&mut self) {
        // SAFETY: `self.fd` is an owned descriptor that is not used afterwards.
        unsafe { libc::close(self.fd) };
    }
}

/// 一个事件。
fn event(kind: u16, code: u16, value: i32) -> InputEvent {
    let mut time = libc::timeval { tv_sec: 0, tv_usec: 0 };
    // SAFETY: `time` is a valid, writable `timeval`; the timezone argument is
    // not needed for UTC.
    unsafe {
        libc::gettimeofday(&mut time, std::ptr::null_mut());
    }
    InputEvent {
        time,
        kind,
        code,
        value,
    }
}

/// 把一个坐标轴的值裁到 `[0, extent - 1]`。
fn clamp_axis(value: f64, extent: u16) -> i32 {
    let max = f64::from(extent.saturating_sub(1));
    value.round().clamp(0.0, max) as i32
}

/// 最近的系统错误。
fn last_error() -> std::io::Error {
    std::io::Error::last_os_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_axis_is_clamped_to_the_desktop() {
        assert_eq!(clamp_axis(-10.0, 1920), 0);
        assert_eq!(clamp_axis(1919.4, 1920), 1919);
        assert_eq!(clamp_axis(5000.0, 1920), 1919);
    }

    #[test]
    fn the_uinput_ioctl_numbers_match_the_kernel_header() {
        // `/usr/include/linux/uinput.h`：`_IOC(dir, type, nr, size)`。
        // 数字错了内核一律回 EINVAL，虚拟设备根本建不起来。
        assert_eq!(UI_SET_EVBIT, 0x4004_5564);
        assert_eq!(UI_SET_KEYBIT, 0x4004_5565);
        assert_eq!(UI_SET_RELBIT, 0x4004_5566);
        assert_eq!(UI_SET_ABSBIT, 0x4004_5567);
        assert_eq!(UI_SET_PROPBIT, 0x4004_556e);
        assert_eq!(UI_DEV_SETUP, 0x405c_5503);
        assert_eq!(UI_ABS_SETUP, 0x401c_5504);
        assert_eq!(UI_DEV_CREATE, 0x5501);
    }

    #[test]
    fn the_absolute_axis_setup_has_the_kernel_layout() {
        // `struct uinput_abs_setup`：`code` + 填充 + `struct input_absinfo`。
        assert_eq!(std::mem::size_of::<UinputAbsSetup>(), 28);
        assert_eq!(std::mem::size_of::<UinputSetup>(), 92);
    }

    #[test]
    fn fractional_scroll_keeps_the_notch_count() {
        // uinput 的 `REL_WHEEL` 是旧式滚轮：一个单位就是一格。
        let mut remainder = 0.0;
        assert_eq!(wheel_notches(&mut remainder, 0.4), 0);
        assert_eq!(wheel_notches(&mut remainder, 0.4), 1);
        // 整格滚轮不多不少。
        let mut remainder = 0.0;
        assert_eq!(wheel_notches(&mut remainder, 1.0), 1);
        assert_eq!(wheel_notches(&mut remainder, -1.0), -1);
    }

    #[test]
    fn the_injector_registers_a_virtual_device() {
        // 需要能写 `/dev/uinput`（桌面会话里的用户有权限，构建机上通常没有）：
        // 没有权限就跳过，有权限时这条测试能抓住 ioctl 参数用错的问题。
        if std::fs::OpenOptions::new().write(true).open("/dev/uinput").is_err() {
            return;
        }
        let name = "Oink injector test";
        let injector = Injector::open(name, (1920, 1080)).expect("建虚拟输入设备");
        let registered = std::fs::read_dir("/sys/class/input")
            .expect("读 /sys/class/input")
            .filter_map(Result::ok)
            .any(|entry| std::fs::read_to_string(entry.path().join("name")).is_ok_and(|value| value.trim() == name));
        drop(injector);
        assert!(registered, "虚拟设备应该出现在 /sys/class/input 里");
    }
}
