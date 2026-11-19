//! XTEST 注入：把 Oink 的事件写进 X 服务器。
//!
//! - 绝对移动按**根窗口像素**发送：XTEST 的绝对坐标是设备单位，实测 1280x1024 的屏幕上
//!   `detail = 0`、坐标 640 落在像素 640（不是 0..=65535 的归一化值），服务器把越界坐标裁到屏幕内。
//! - 相对移动按像素发送（`detail = 1`）。
//! - 按键、鼠标键、滚轮都走 XTEST 的假事件。
//!
//! 自己注入的事件也会被 RECORD 记下来：注入前先把它们登记到台账（[`Pending`]），
//! 截获线程读到时跳过，否则会形成回环。

use oink_foundation::Point;
use oink_foundation::input::{InjectEvent, KeyCode, MouseButton, ScrollUnit};

use x11rb::protocol::xproto;
use x11rb::protocol::xtest::ConnectionExt as _;

use crate::InputError;
use crate::linux::{Inner, Pending, clamp_to_root, lock};
use crate::linux_keymap as keymap;

/// 注入一个事件。
pub(crate) fn inject(inner: &Inner, event: &InjectEvent) -> Result<(), InputError> {
    match event {
        InjectEvent::MoveTo { pos } => move_to(inner, *pos),
        InjectEvent::MoveToWithDelta { pos, .. } => move_to(inner, *pos),
        InjectEvent::MoveRelative { dx, dy } => move_relative(inner, *dx, *dy),
        InjectEvent::Button { button, pressed } => mouse_button(inner, *button, *pressed),
        InjectEvent::Key { code, pressed } => key(inner, *code, *pressed),
        // 远端来源不要求 Win+L 补偿的按键：正常注入即可。
        InjectEvent::KeyWithoutLock { code, pressed } => key(inner, *code, *pressed),
        InjectEvent::Scroll { dx, dy, unit, .. } => scroll(inner, *dx, *dy, *unit),
        // 手势、触摸板多点与触摸板滚动在 X11 上没有对应的原生事件：
        // XTEST 只能造键鼠事件，无法表达多点触控。
        InjectEvent::Gesture(_) => Err(InputError::Unsupported),
        InjectEvent::TouchpadContacts(_) => Err(InputError::Unsupported),
        InjectEvent::TouchpadScroll { .. } => Err(InputError::Unsupported),
        // 音量在 X11 上不是输入事件：媒体键是键盘事件，已经在 `Key` 里处理。
        InjectEvent::Volume { .. } => Err(InputError::Unsupported),
    }
}

/// 绝对移动：把指针放到 `pos`（根窗口像素）。
fn move_to(inner: &Inner, pos: Point) -> Result<(), InputError> {
    let (x, y) = clamp_to_root(inner, pos);
    fake_motion(inner, x, y)
}

/// 相对移动：按像素移动。
fn move_relative(inner: &Inner, dx: f64, dy: f64) -> Result<(), InputError> {
    let x = clamp_delta(dx);
    let y = clamp_delta(dy);
    let pending = Pending::Motion { x, y };
    lock(&inner.state).push_pending(pending);
    let result = fake_motion_relative(inner, x, y);
    if result.is_err() {
        lock(&inner.state).drop_pending(pending);
    }
    result
}

/// 鼠标按键。
fn mouse_button(inner: &Inner, button: MouseButton, pressed: bool) -> Result<(), InputError> {
    let number = keymap::x11_button(button);
    let pending = Pending::Button {
        button: number,
        pressed,
    };
    lock(&inner.state).push_pending(pending);
    let result = fake_button(inner, number, pressed);
    if result.is_err() {
        lock(&inner.state).drop_pending(pending);
    }
    result
}

/// 按键：HID Usage → keysym → 当前键盘映射里的键码。
fn key(inner: &Inner, code: KeyCode, pressed: bool) -> Result<(), InputError> {
    let Some(keysym) = keymap::keysym_from_hid(code) else {
        return Err(InputError::Platform(format!("X11 没有这个键：{code:?}")));
    };
    let keycode = {
        let mut x11 = lock(&inner.x11);
        // 每次注入都重新读一次键盘映射：布局换了以后立刻生效（正确性优先于缓存）。
        x11.refresh_map()?;
        x11.map
            .keycode(keysym)
            .ok_or_else(|| InputError::Platform(format!("当前键盘布局里没有这个键：{code:?}")))?
    };
    let pending = Pending::Key { keycode, pressed };
    lock(&inner.state).push_pending(pending);
    let result = fake_key(inner, keycode, pressed);
    if result.is_err() {
        lock(&inner.state).drop_pending(pending);
    }
    result
}

/// 滚动：每一格滚轮是一个「按下后立刻抬起」的按钮对。
///
/// X11 的滚轮按钮只有整格：小数滚轮的零头留到下次，滚动量与来源一致
/// （四舍五入成整格会把一小段滑动放大成整格，滚动因此快好几倍）。
fn scroll(inner: &Inner, dx: f64, dy: f64, unit: ScrollUnit) -> Result<(), InputError> {
    let (dx, dy) = keymap::scroll_notches(dx, dy, unit);
    let (vertical, horizontal) = {
        let mut state = lock(&inner.state);
        (
            keymap::take_whole(&mut state.scroll_remainder.0, dy),
            keymap::take_whole(&mut state.scroll_remainder.1, dx),
        )
    };
    for number in keymap::notch_buttons(horizontal as f64, vertical as f64) {
        let press = Pending::Button {
            button: number,
            pressed: true,
        };
        let release = Pending::Button {
            button: number,
            pressed: false,
        };
        {
            let mut state = lock(&inner.state);
            state.push_pending(press);
        }
        if let Err(error) = fake_button(inner, number, true) {
            lock(&inner.state).drop_pending(press);
            return Err(error);
        }
        {
            let mut state = lock(&inner.state);
            state.push_pending(release);
        }
        fake_button(inner, number, false)?;
    }
    Ok(())
}

/// 把相对位移裁到 `i16` 能表示的范围（XTEST 的相对移动是 `i16` 像素）。
fn clamp_delta(value: f64) -> i16 {
    value.round().clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

/// XTEST 假事件：绝对移动。
pub(crate) fn fake_motion(inner: &Inner, x: i16, y: i16) -> Result<(), InputError> {
    let x11 = lock(&inner.x11);
    let root = x11.root();
    x11.conn
        .xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 0, 0, root, x, y, 0)
        .map_err(|e| InputError::Platform(format!("XTEST 移动失败：{e}")))?;
    Ok(())
}

/// XTEST 假事件：相对移动（像素）。
pub(crate) fn fake_motion_relative(inner: &Inner, x: i16, y: i16) -> Result<(), InputError> {
    let x11 = lock(&inner.x11);
    let root = x11.root();
    x11.conn
        .xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 1, 0, root, x, y, 0)
        .map_err(|e| InputError::Platform(format!("XTEST 移动失败：{e}")))?;
    Ok(())
}

/// XTEST 假事件：鼠标按键（`detail` 是按钮号）。
fn fake_button(inner: &Inner, button: u8, pressed: bool) -> Result<(), InputError> {
    let kind = if pressed {
        xproto::BUTTON_PRESS_EVENT
    } else {
        xproto::BUTTON_RELEASE_EVENT
    };
    let x11 = lock(&inner.x11);
    x11.conn
        .xtest_fake_input(kind, button, 0, 0, 0, 0, 0)
        .map_err(|e| InputError::Platform(format!("XTEST 鼠标按键失败：{e}")))?;
    Ok(())
}

/// XTEST 假事件：按键（`detail` 是键码）。
fn fake_key(inner: &Inner, keycode: u8, pressed: bool) -> Result<(), InputError> {
    let kind = if pressed {
        xproto::KEY_PRESS_EVENT
    } else {
        xproto::KEY_RELEASE_EVENT
    };
    let x11 = lock(&inner.x11);
    x11.conn
        .xtest_fake_input(kind, keycode, 0, 0, 0, 0, 0)
        .map_err(|e| InputError::Platform(format!("XTEST 按键失败：{e}")))?;
    Ok(())
}
