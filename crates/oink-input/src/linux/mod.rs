//! Linux（X11）实现：XRecord 截获、XTEST 注入。
//!
//! - 截获：XRecord 扩展。RECORD 需要两条连接：控制连接（建立 / 关闭 record context）与数据连接
//!   （读被记录的事件）。要截获的设备事件在 `device_events` 范围里按原始协议格式送过来。
//! - 注入：XTEST 扩展。绝对移动按**根窗口像素**发送：XTEST 的绝对坐标是设备单位，实测
//!   1280x1024 的屏幕上 `detail = 0`、坐标 640 落在像素 640（不是 0..=65535 的归一化值），
//!   服务器把越界的坐标裁到屏幕内。
//! - 键码：HID ↔ keysym，见 [`crate::linux_keymap`]。
//! - 坐标：根窗口像素，y 向下（与 Windows 的全局坐标约定一致）。
//! - 自己注入的事件也会被 RECORD 记下来，截获线程按注入台账过滤，否则会形成回环。
//! - X11 没有事件过滤钩子，XRecord 是被动记录：截获回调的返回值被忽略，本机照常处理事件。

mod capture;
mod inject;

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use oink_foundation::input::{CaptureMode, FeelProfile, InjectEvent, InputPermissions, PermissionState};
use oink_foundation::{Point, Rect};

use crate::capture_loop;
use crate::linux_keymap as keymap;
use crate::{EventHandler, InputBackend, InputError};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::record;
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::xtest;
use x11rb::rust_connection::RustConnection;

/// X11 连接与键盘映射。
pub(crate) struct X11 {
    /// 控制连接：XTEST 注入、查询、RECORD context 的建立与关闭。
    pub(crate) conn: RustConnection,
    /// 连接选中的屏幕序号。
    pub(crate) screen: usize,
    /// 当前键盘映射（键码 ↔ keysym）。
    pub(crate) map: keymap::KeyboardMap,
}

impl X11 {
    /// 根窗口。
    pub(crate) fn root(&self) -> xproto::Window {
        self.conn.setup().roots[self.screen].root
    }

    /// 根窗口的像素尺寸（X11 的「屏幕」就是根窗口，多显示器时覆盖整个桌面）。
    pub(crate) fn size(&self) -> (u16, u16) {
        let root = &self.conn.setup().roots[self.screen];
        (root.width_in_pixels, root.height_in_pixels)
    }

    /// 重新读取键盘映射（键码 → keysym）。
    pub(crate) fn refresh_map(&mut self) -> Result<(), InputError> {
        let setup = self.conn.setup();
        let min = setup.min_keycode;
        let count = setup.max_keycode.saturating_sub(min).saturating_add(1);
        let reply = self
            .conn
            .get_keyboard_mapping(min, count)
            .map_err(|e| InputError::Platform(format!("读取键盘映射失败：{e}")))?
            .reply()
            .map_err(|e| InputError::Platform(format!("读取键盘映射失败：{e}")))?;
        self.map = keymap::KeyboardMap {
            min_keycode: min,
            per_keycode: reply.keysyms_per_keycode,
            keysyms: reply.keysyms,
        };
        Ok(())
    }
}

/// 后端的共享状态。
pub(crate) struct Inner {
    /// X11 连接与键盘映射。
    pub(crate) x11: Mutex<X11>,
    /// 截获状态。
    pub(crate) state: Mutex<State>,
    /// 截获线程与 RECORD context。
    pub(crate) capture: Mutex<CaptureSlot>,
}

/// 截获状态：模式、回调、上一次的位置与自己注入的台账。
pub(crate) struct State {
    /// 当前的截获模式。
    pub(crate) mode: CaptureMode,
    /// 截获线程是否在跑。
    pub(crate) running: bool,
    /// 上一次观测到的根窗口坐标。
    pub(crate) last_root: Option<(i16, i16)>,
    /// `Remote` 模式的停靠点（根窗口坐标）。
    pub(crate) park: Option<(i16, i16)>,
    /// 自己注入、等待被截获线程过滤掉的事件。
    pub(crate) pending: VecDeque<Pending>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            mode: CaptureMode::Local,
            running: false,
            last_root: None,
            park: None,
            pending: VecDeque::new(),
        }
    }
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("State")
            .field("mode", &self.mode)
            .field("running", &self.running)
            .field("last_root", &self.last_root)
            .field("park", &self.park)
            .field("pending", &self.pending.len())
            .finish()
    }
}

/// 自己注入的事件：截获线程读到它们时必须跳过，否则会形成回环。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    /// 按键（X11 键码）。
    Key {
        /// X11 键码。
        keycode: u8,
        /// 是按下还是抬起。
        pressed: bool,
    },
    /// 鼠标按键（X11 按钮号）。
    Button {
        /// X11 按钮号。
        button: u8,
        /// 是按下还是抬起。
        pressed: bool,
    },
    /// 指针移动（根窗口坐标）。
    Motion {
        /// 根窗口横坐标。
        x: i16,
        /// 根窗口纵坐标。
        y: i16,
    },
}

/// 注入台账：自己写进 X11 的事件，截获线程读到时跳过。
const PENDING_LIMIT: usize = 64;
/// 注入的移动与截获到的位置允许差几个像素（XTEST 的绝对坐标会被服务器量化）。
const MOTION_TOLERANCE: i16 = 2;

impl State {
    /// 登记一个自己注入的事件。
    pub(crate) fn push_pending(&mut self, pending: Pending) {
        if self.pending.len() >= PENDING_LIMIT {
            self.pending.pop_front();
        }
        self.pending.push_back(pending);
    }

    /// 撤掉一个登记（注入失败时用）。
    pub(crate) fn drop_pending(&mut self, pending: Pending) {
        if let Some(index) = self.pending.iter().position(|p| *p == pending) {
            self.pending.remove(index);
        }
    }

    /// 截获线程读到按键：是自己注入的就返回 `true`（应当跳过）。
    pub(crate) fn take_pending_key(&mut self, keycode: u8, pressed: bool) -> bool {
        self.take_pending(
            |pending| matches!(pending, Pending::Key { keycode: k, pressed: p } if k == keycode && p == pressed),
        )
    }

    /// 截获线程读到鼠标按键：是自己注入的就返回 `true`（应当跳过）。
    pub(crate) fn take_pending_button(&mut self, button: u8, pressed: bool) -> bool {
        self.take_pending(
            |pending| matches!(pending, Pending::Button { button: b, pressed: p } if b == button && p == pressed),
        )
    }

    /// 截获线程读到移动：是自己注入的就返回 `true`（应当跳过）。
    pub(crate) fn take_pending_motion(&mut self, x: i16, y: i16) -> bool {
        self.take_pending(|pending| match pending {
            Pending::Motion { x: px, y: py } => {
                (px - x).abs() <= MOTION_TOLERANCE && (py - y).abs() <= MOTION_TOLERANCE
            }
            _ => false,
        })
    }

    /// 找出第一个匹配的登记并撤掉它。
    fn take_pending(&mut self, matches: impl Fn(Pending) -> bool) -> bool {
        match self.pending.iter().position(|pending| matches(*pending)) {
            Some(index) => {
                self.pending.remove(index);
                true
            }
            None => false,
        }
    }
}

/// 截获线程与 RECORD context。
#[derive(Debug, Default)]
pub(crate) struct CaptureSlot {
    /// RECORD context 的 XID。
    pub(crate) context: Option<u32>,
    /// 读 RECORD 数据的线程。
    pub(crate) reader: Option<std::thread::JoinHandle<()>>,
    /// 派发事件的线程（路由模式：路由的 owner 在这一条线程上）。
    pub(crate) dispatcher: Option<std::thread::JoinHandle<()>>,
    /// 停止截获时通知派发线程。
    pub(crate) stop: Option<std::sync::mpsc::Sender<capture::Message>>,
}

/// Linux（X11）输入后端。
pub(crate) struct LinuxInput {
    inner: Arc<Inner>,
}

impl fmt::Debug for LinuxInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinuxInput").finish_non_exhaustive()
    }
}

/// 取锁。锁中毒时照常取用：截获线程 panic 不应该让整个输入后端失效。
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 用 XTEST 把指针挪到根窗口坐标 `(x, y)`。
///
/// 自己写入的位置先登记到台账：截获线程读到这条移动时必须跳过，否则会把自己写的位置
/// 当成用户移动报上去（`Remote` 模式下就会形成回环）。
pub(crate) fn warp_pointer(inner: &Inner, x: i16, y: i16) -> Result<(), InputError> {
    let pending = Pending::Motion { x, y };
    lock(&inner.state).push_pending(pending);
    let result = inject::fake_motion(inner, x, y);
    if result.is_err() {
        lock(&inner.state).drop_pending(pending);
    }
    result
}

/// 根窗口的像素尺寸。
pub(crate) fn root_size(inner: &Inner) -> (u16, u16) {
    lock(&inner.x11).size()
}

/// 把本机坐标裁到根窗口范围内。
pub(crate) fn clamp_to_root(inner: &Inner, point: Point) -> (i16, i16) {
    let (width, height) = root_size(inner);
    (clamp_axis(point.x, width), clamp_axis(point.y, height))
}

/// 把一个坐标轴的值裁到 `[0, extent - 1]`，并落在 `i16` 能表示的范围内。
fn clamp_axis(value: f64, extent: u16) -> i16 {
    let max = f64::from(extent.saturating_sub(1)).min(f64::from(i16::MAX));
    value.round().clamp(0.0, max) as i16
}

/// 创建本机（Linux / X11）的实现。
pub(crate) fn backend() -> Result<Box<dyn InputBackend>, InputError> {
    let (conn, screen) =
        RustConnection::connect(None).map_err(|e| InputError::Platform(format!("连接 X 服务器失败：{e}")))?;
    // RECORD 用来截获，XTEST 用来注入：缺一个就没法工作。
    if conn
        .extension_information(record::X11_EXTENSION_NAME)
        .map_err(|e| InputError::Platform(format!("查询 X 扩展失败：{e}")))?
        .is_none()
    {
        return Err(InputError::Platform("X 服务器没有 RECORD 扩展，无法截获输入".into()));
    }
    if conn
        .extension_information(xtest::X11_EXTENSION_NAME)
        .map_err(|e| InputError::Platform(format!("查询 X 扩展失败：{e}")))?
        .is_none()
    {
        return Err(InputError::Platform("X 服务器没有 XTEST 扩展，无法注入输入".into()));
    }
    let inner = Arc::new(Inner {
        x11: Mutex::new(X11 {
            conn,
            screen,
            map: keymap::KeyboardMap::default(),
        }),
        state: Mutex::new(State::default()),
        capture: Mutex::new(CaptureSlot::default()),
    });
    Ok(Box::new(LinuxInput { inner }))
}

impl InputBackend for LinuxInput {
    fn start(&self, handler: EventHandler) -> Result<(), InputError> {
        capture::start(&self.inner, handler)
    }

    fn start_loop(&self, owner: Box<dyn capture_loop::CaptureLoop>) -> Result<(), InputError> {
        capture::start_loop(&self.inner, owner)
    }

    fn stop(&self) {
        capture::stop(&self.inner);
    }

    fn capture_active(&self) -> Option<bool> {
        Some(lock(&self.inner.state).running)
    }

    fn set_mode(&self, mode: CaptureMode) -> Result<(), InputError> {
        let park = match mode {
            CaptureMode::Local => None,
            CaptureMode::Remote { park } => Some(clamp_to_root(&self.inner, park)),
        };
        {
            let mut state = lock(&self.inner.state);
            state.mode = mode;
            state.park = park;
            if let Some((x, y)) = park {
                // 停靠点就是之后位移的基准。
                state.last_root = Some((x, y));
            }
        }
        if let Some((x, y)) = park {
            // X11 没有「隐藏光标」：用 XTEST 把指针挪到停靠点，
            // 之后的移动由截获线程挪回来（见 `capture.rs`）。
            warp_pointer(&self.inner, x, y)?;
        }
        Ok(())
    }

    fn inject(&self, event: &InjectEvent) -> Result<(), InputError> {
        inject::inject(&self.inner, event)
    }

    fn restore_local(&self, at: Point) -> Result<(), InputError> {
        // 离开 `Remote`：先切回 `Local`（本机重新处理输入），再把指针放到 `at`。
        self.set_mode(CaptureMode::Local)?;
        self.inject(&InjectEvent::MoveTo { pos: at })
    }

    fn cursor_position(&self) -> Result<Point, InputError> {
        let x11 = lock(&self.inner.x11);
        let root = x11.root();
        let reply = x11
            .conn
            .query_pointer(root)
            .map_err(|e| InputError::Platform(format!("查询光标位置失败：{e}")))?
            .reply()
            .map_err(|e| InputError::Platform(format!("查询光标位置失败：{e}")))?;
        Ok(Point::new(f64::from(reply.root_x), f64::from(reply.root_y)))
    }

    fn system_cursor_clip(&self) -> Result<Option<Rect>, InputError> {
        // X11 没有应用级的指针禁区：指针只受根窗口的像素边界限制，那不是
        // 「系统把指针关在某个矩形里」。报告根窗口会让路由把整块屏幕当成
        // 一个禁区，于是屏幕边沿永远到不了（接管别的设备就不会触发），
        // 远端送来的移动也会被夹回屏幕里。
        Ok(None)
    }

    fn feel_profile(&self) -> FeelProfile {
        FeelProfile {
            key_repeat_delay_ms: 500,
            key_repeat_interval_ms: 33,
            // X11 的默认滚轮是每格 3 行。
            wheel_lines_per_notch: Some(3.0),
            natural_scroll_mouse: false,
            natural_scroll_touchpad: false,
        }
    }

    fn permissions(&self) -> InputPermissions {
        InputPermissions {
            // X11 没有对应的权限开关：能不能截获 / 注入取决于服务器有没有 RECORD / XTEST，
            // 那由 `backend()` 与 `start()` 报错。
            accessibility: PermissionState::Granted,
            input_monitoring: PermissionState::NotApplicable,
            secure_input_active: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// X11 没有应用级的指针禁区：根窗口不是禁区。
    ///
    /// 需要真的 X 服务器：容器里的测试脚本会起一个 Xvfb。
    #[test]
    fn the_pointer_is_not_confined_to_a_region() {
        if std::env::var_os("DISPLAY").is_none() {
            // 没有 X 会话就没有指针。
            return;
        }
        let Ok(backend) = backend() else {
            return;
        };
        assert_eq!(
            backend.system_cursor_clip().expect("指针禁区"),
            None,
            "X11 上没有应用级的指针禁区"
        );
    }
}
