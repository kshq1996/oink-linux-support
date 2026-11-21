//! Wayland 会话的输入实现：uinput 注入 + evdev 截获。
//!
//! Wayland 会话里 X11 只是「客人」：合成器掌握真正的指针，XTEST 挪不动屏幕上的光标
//! （实测 `xdotool mousemove` 之后 X11 指针还在原处），XRecord 也录不到指针移动。
//! 所以 Wayland 上换一套原生做法：
//!
//! - 注入：[`super::uinput`] —— 一个绝对定位的虚拟设备，光标放到哪个像素就写哪个坐标。
//! - 截获：[`super::evdev`] —— 直接读内核的输入设备。
//!
//! 桌面尺寸（逻辑像素）从 XWayland 的根窗口取：XWayland 的根窗口覆盖整个 Wayland
//! 桌面，尺寸就是逻辑像素的桌面尺寸。没有 XWayland 时退回 1920x1080。

use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use oink_foundation::Point;
use oink_foundation::input::{
    CaptureMode, CapturedEvent, FeelProfile, InjectEvent, InputEvent, InputPermissions, KeyCode, MouseButton,
    PermissionState, ScrollPhase, ScrollUnit, SourceKind,
};

use crate::capture_loop::{CaptureLoop, Wake};
use crate::linux::evdev::{self, Device};
use crate::linux::keycodes;
use crate::linux::uinput::Injector;
use crate::{EventHandler, InputBackend, InputError};
use x11rb::connection::Connection;

/// 虚拟设备的名字：截获时按名字跳过自己注入的设备。
pub(crate) const DEVICE_NAME: &str = "Oink Virtual Pointer";

/// 桌面尺寸的兜底值（没有 XWayland 时用）。
const FALLBACK_SIZE: (u16, u16) = (1920, 1080);

/// 截获线程一次读多少字节。
const READ_BUFFER: usize = evdev::EVENT_SIZE * 64;

/// 派发线程收到的一条消息。
enum Message {
    /// 截获到的物理事件。
    Event(Box<CapturedEvent>),
    /// 路由有活干：醒来重新 `poll`。
    Wake,
    /// 停止截获。
    Stop,
    /// 读取线程没能读设备：截获不可用。
    CaptureFailed,
    /// 读取线程已经在读设备：截获可用。
    CaptureReady,
}

/// 截获状态：模式、追踪到的指针位置与停靠点。
struct State {
    /// 当前的截获模式。
    mode: CaptureMode,
    /// 截获线程是否在跑。
    running: bool,
    /// 追踪到的指针位置（逻辑像素）。
    cursor: Point,
    /// `Remote` 模式的停靠点。
    park: Option<Point>,
}

impl State {
    /// 新建状态：指针位置先放在桌面中间。
    ///
    /// Wayland 拿不到光标的真实位置（合成器不告诉应用），先从中点开始，
    /// 第一次移动之后就是准的。
    fn new(size: (u16, u16)) -> Self {
        Self {
            mode: CaptureMode::Local,
            running: false,
            cursor: Point::new(f64::from(size.0) / 2.0, f64::from(size.1) / 2.0),
            park: None,
        }
    }
}

/// 截获线程。
#[derive(Default)]
struct CaptureSlot {
    /// 读设备的线程。
    reader: Option<JoinHandle<()>>,
    /// 派发事件的线程（路由模式：路由的 owner 在这一条线程上）。
    dispatcher: Option<JoinHandle<()>>,
    /// 停止截获时通知派发线程。
    stop: Option<Sender<Message>>,
    /// 让读取线程知道该收工了。
    flag: Option<Arc<AtomicBool>>,
}
/// 后端的共享状态。
struct Inner {
    /// XWayland 连接：只用来读桌面尺寸。
    x11: Mutex<Option<x11rb::rust_connection::RustConnection>>,
    /// 虚拟输入设备（注入）。
    injector: Mutex<Option<Injector>>,
    /// 桌面尺寸（逻辑像素）。
    size: Mutex<(u16, u16)>,
    /// 截获状态。
    state: Mutex<State>,
    /// 截获线程。
    capture: Mutex<CaptureSlot>,
    /// 手柄的指针节点：远端接管指针时要独占它们，
    /// 否则摇杆会和注入的指针拉扯。
    gamepad_pointers: Mutex<Vec<evdev::Device>>,
}

/// Wayland 输入后端。
pub(crate) struct Wayland {
    inner: Arc<Inner>,
}

/// 取锁。锁中毒时照常取用：截获线程 panic 不应该让整个输入后端失效。
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 创建 Wayland 会话的实现。
pub(crate) fn backend() -> Result<Box<dyn InputBackend>, InputError> {
    let (x11, size) = connect_x11();
    let injector = Injector::open(DEVICE_NAME, size)?;
    let inner = Arc::new(Inner {
        x11: Mutex::new(x11),
        injector: Mutex::new(Some(injector)),
        size: Mutex::new(size),
        state: Mutex::new(State::new(size)),
        capture: Mutex::new(CaptureSlot::default()),
        gamepad_pointers: Mutex::new(Vec::new()),
    });
    Ok(Box::new(Wayland { inner }))
}

/// 连上 XWayland，读桌面尺寸（逻辑像素）。
///
/// XWayland 的根窗口覆盖整个 Wayland 桌面；没有 XWayland 时用兜底值。
fn connect_x11() -> (Option<x11rb::rust_connection::RustConnection>, (u16, u16)) {
    match x11rb::rust_connection::RustConnection::connect(None) {
        Ok((conn, screen)) => {
            let root = &conn.setup().roots[screen];
            let size = (root.width_in_pixels, root.height_in_pixels);
            (Some(conn), size)
        }
        Err(_) => (None, FALLBACK_SIZE),
    }
}

/// 桌面尺寸（逻辑像素）。
///
/// 分辨率变了要重新读：虚拟设备的绝对范围就是桌面尺寸。
fn desktop_size(inner: &Inner) -> (u16, u16) {
    let guard = lock(&inner.x11);
    if let Some(conn) = guard.as_ref() {
        let screen = conn.setup().roots.len().saturating_sub(1);
        let root = &conn.setup().roots[screen];
        return (root.width_in_pixels, root.height_in_pixels);
    }
    *lock(&inner.size)
}

impl Wayland {
    /// 桌面尺寸（逻辑像素）。
    fn size(&self) -> (u16, u16) {
        desktop_size(&self.inner)
    }

    /// 把指针放到 `pos`（桌面逻辑像素）。
    fn move_to(&self, pos: Point) -> Result<(), InputError> {
        let injector = lock(&self.inner.injector);
        let Some(injector) = injector.as_ref() else {
            return Err(InputError::Platform("虚拟输入设备已经关闭".into()));
        };
        injector.move_to(pos, self.size())
    }
}

impl InputBackend for Wayland {
    fn start(&self, handler: EventHandler) -> Result<(), InputError> {
        start_inner(&self.inner, handler, None)
    }

    fn start_loop(&self, owner: Box<dyn CaptureLoop>) -> Result<(), InputError> {
        start_inner(&self.inner, Arc::new(crate::capture_loop::event), Some(owner))
    }

    fn stop(&self) {
        stop_inner(&self.inner);
    }

    fn capture_active(&self) -> Option<bool> {
        Some(lock(&self.inner.state).running)
    }

    fn set_mode(&self, mode: CaptureMode) -> Result<(), InputError> {
        let park = match mode {
            CaptureMode::Local => None,
            CaptureMode::Remote { park } => Some(clamp_to_desktop(park, self.size())),
        };
        {
            let mut state = lock(&self.inner.state);
            state.mode = mode;
            state.park = park;
            if let Some(park) = park {
                // 停靠点就是之后位移的基准。
                state.cursor = park;
            }
        }
        // 远端接管指针时，手柄的指针节点要从桌面手里拿走：
        // 桌面把摇杆当鼠标，摇杆一动就把注入的指针拖回去。
        take_over_gamepad_pointers(&self.inner, park.is_some());
        if let Some(park) = park {
            // Wayland 没有「隐藏光标」：把指针挪到停靠点，
            // 之后的本地移动由截获线程挪回来。
            self.move_to(park)?;
        }
        Ok(())
    }

    fn inject(&self, event: &InjectEvent) -> Result<(), InputError> {
        inject(&self.inner, event)
    }

    fn restore_local(&self, at: Point) -> Result<(), InputError> {
        // 离开 `Remote`：先切回 `Local`（本机重新处理输入），再把指针放到 `at`。
        self.set_mode(CaptureMode::Local)?;
        self.inject(&InjectEvent::MoveTo { pos: at })
    }

    fn cursor_position(&self) -> Result<Point, InputError> {
        Ok(lock(&self.inner.state).cursor)
    }

    fn system_cursor_clip(&self) -> Result<Option<oink_foundation::Rect>, InputError> {
        // Wayland 没有应用级的指针禁区。
        Ok(None)
    }

    fn feel_profile(&self) -> FeelProfile {
        FeelProfile {
            key_repeat_delay_ms: 500,
            key_repeat_interval_ms: 33,
            wheel_lines_per_notch: Some(3.0),
            natural_scroll_mouse: false,
            natural_scroll_touchpad: false,
        }
    }

    fn permissions(&self) -> InputPermissions {
        InputPermissions {
            // 能不能截获 / 注入取决于 `/dev/input` 与 `/dev/uinput` 的权限，
            // 那由 `backend()` 与 `start()` 报错。
            accessibility: PermissionState::Granted,
            input_monitoring: PermissionState::Granted,
            secure_input_active: false,
        }
    }
}

/// 注入一个事件。
fn inject(inner: &Arc<Inner>, event: &InjectEvent) -> Result<(), InputError> {
    let size = desktop_size(inner);
    match event {
        InjectEvent::MoveTo { pos } => move_to(inner, *pos, size),
        InjectEvent::MoveToWithDelta { pos, .. } => move_to(inner, *pos, size),
        InjectEvent::MoveRelative { dx, dy } => move_relative(inner, *dx, *dy),
        InjectEvent::Button { button, pressed } => with_injector(inner, |injector| injector.button(*button, *pressed)),
        InjectEvent::Key { code, pressed } => with_injector(inner, |injector| injector.key(*code, *pressed)),
        // 远端来源不要求 Win+L 补偿的按键：正常注入即可。
        InjectEvent::KeyWithoutLock { code, pressed } => with_injector(inner, |injector| injector.key(*code, *pressed)),
        InjectEvent::Scroll { dx, dy, unit, .. } => with_injector(inner, |injector| injector.scroll(*dx, *dy, *unit)),
        // 手势、触摸板多点与触摸板滚动在 Linux 上没有对应的原生事件。
        InjectEvent::Gesture(_) => Err(InputError::Unsupported),
        InjectEvent::TouchpadContacts(_) => Err(InputError::Unsupported),
        InjectEvent::TouchpadScroll { .. } => Err(InputError::Unsupported),
        // 音量不是输入事件：媒体键是键盘事件，已经在 `Key` 里处理。
        InjectEvent::Volume { .. } => Err(InputError::Unsupported),
    }
}

/// 用虚拟设备做一件事。
fn with_injector<T>(
    inner: &Arc<Inner>,
    action: impl FnOnce(&Injector) -> Result<T, InputError>,
) -> Result<T, InputError> {
    let mut guard = lock(&inner.injector);
    let size = desktop_size(inner);
    // 设备的绝对范围是建设备时定下的：分辨率变了要重建一个。
    if guard.as_ref().is_some_and(|injector| injector.range() != size) {
        *guard = None;
    }
    if guard.is_none() {
        *guard = Some(Injector::open(DEVICE_NAME, size)?);
    }
    let injector = guard
        .as_ref()
        .ok_or_else(|| InputError::Platform("虚拟输入设备已经关闭".into()))?;
    action(injector)
}

/// 绝对移动：把指针放到 `pos`（桌面逻辑像素）。
fn move_to(inner: &Arc<Inner>, pos: Point, size: (u16, u16)) -> Result<(), InputError> {
    with_injector(inner, |injector| injector.move_to(pos, size))?;
    // 合成器的光标被放到哪，截获线程就知道它在哪：Wayland 没有别的办法
    // 读到光标的绝对位置，注入是我们唯一准确的位置来源。
    lock(&inner.state).cursor = clamp_to_desktop(pos, size);
    Ok(())
}

/// 相对移动：虚拟设备是绝对定位的，相对位移要自己换算成绝对坐标。
fn move_relative(inner: &Arc<Inner>, dx: f64, dy: f64) -> Result<(), InputError> {
    let cursor = lock(&inner.state).cursor;
    let target = Point::new(cursor.x + dx, cursor.y + dy);
    move_to(inner, target, desktop_size(inner))
}
/// 独占（或放开）手柄的指针节点。
///
/// 桌面（libinput）把手柄的指针节点当成鼠标：摇杆一动，光标就跟着动，
/// 和注入的指针互相拉扯，光标像是被摇杆拖回原处。
/// 远端接管指针期间独占这些节点，桌面就收不到它们的事件；
/// 本地拿回指针时再放开。
fn take_over_gamepad_pointers(inner: &Arc<Inner>, take_over: bool) {
    let devices = lock(&inner.gamepad_pointers);
    let mut failed = Vec::new();
    for device in devices.iter() {
        if !evdev::grab(device, take_over) {
            failed.push(device.name.clone());
        }
    }
    // 独占失败不留痕迹就看不出摇杆为什么还在拖光标，所以写下来。
    tracing::info!(
        take_over,
        devices = devices.len(),
        failed = ?failed,
        "手柄指针节点的独占"
    );
}

/// 开始截获：打开输入设备、起读取线程与派发线程。
fn start_inner(
    inner: &Arc<Inner>,
    handler: EventHandler,
    owner: Option<Box<dyn CaptureLoop>>,
) -> Result<(), InputError> {
    {
        let slot = lock(&inner.capture);
        if slot.reader.is_some() {
            return Err(InputError::Platform("截获已经在运行".into()));
        }
    }
    // 手柄的指针节点不参与截获，但要留着：远端接管指针时要独占它们。
    let mut gamepad_pointers = Vec::new();
    let mut devices = Vec::new();
    for device in evdev::open_devices() {
        if device.name == DEVICE_NAME {
            continue;
        }
        if device.gamepad_pointer {
            gamepad_pointers.push(device);
            continue;
        }
        if device.pointer || device.buttons || device.keyboard {
            devices.push(device);
        }
    }
    if devices.is_empty() {
        return Err(InputError::Platform(
            "读不到任何输入设备（/dev/input）：截获不可用".into(),
        ));
    }
    *lock(&inner.gamepad_pointers) = gamepad_pointers;
    take_over_gamepad_pointers(inner, matches!(lock(&inner.state).mode, CaptureMode::Remote { .. }));
    {
        let mut state = lock(&inner.state);
        state.running = true;
    }
    let (tx, rx) = mpsc::channel::<Message>();
    // 唤醒句柄：路由有活干时叫醒派发线程。
    let wake_tx = tx.clone();
    let wake: Wake = Arc::new(move || {
        let _ = wake_tx.send(Message::Wake);
    });
    // 唤醒源在这里就装好，不等派发线程：路由线程紧接着发出的第一条命令
    // 不能撞上「还没有唤醒源」的窗口（那次会 panic，并把整个后台进程带崩）。
    let mut owner = owner;
    if let Some(owner) = owner.as_mut() {
        owner.set_wake(wake.clone());
    }
    let flag = Arc::new(AtomicBool::new(false));
    let reader = std::thread::Builder::new().name("oink-evdev-read".to_string()).spawn({
        let inner = Arc::clone(inner);
        let tx = tx.clone();
        let flag = Arc::clone(&flag);
        move || read_devices(inner, devices, tx, flag)
    });
    let reader = match reader {
        Ok(reader) => reader,
        Err(error) => {
            let mut state = lock(&inner.state);
            state.running = false;
            return Err(InputError::Platform(format!("启动截获线程失败：{error}")));
        }
    };
    let dispatcher = std::thread::Builder::new()
        .name("oink-evdev-dispatch".to_string())
        .spawn({
            let inner = Arc::clone(inner);
            move || dispatch(inner, handler, owner, rx)
        });
    let dispatcher = match dispatcher {
        Ok(dispatcher) => dispatcher,
        Err(error) => {
            flag.store(true, Ordering::Relaxed);
            let _ = reader.join();
            let mut state = lock(&inner.state);
            state.running = false;
            return Err(InputError::Platform(format!("启动派发线程失败：{error}")));
        }
    };
    let mut slot = lock(&inner.capture);
    slot.reader = Some(reader);
    slot.dispatcher = Some(dispatcher);
    slot.stop = Some(tx);
    slot.flag = Some(flag);
    Ok(())
}

/// 停止截获：通知两条线程并 join。
fn stop_inner(inner: &Arc<Inner>) {
    let (reader, dispatcher, stop, flag) = {
        let mut slot = lock(&inner.capture);
        (
            slot.reader.take(),
            slot.dispatcher.take(),
            slot.stop.take(),
            slot.flag.take(),
        )
    };
    if let Some(flag) = flag {
        flag.store(true, Ordering::Relaxed);
    }
    if let Some(stop) = stop {
        let _ = stop.send(Message::Stop);
    }
    if let Some(reader) = reader {
        let _ = reader.join();
    }
    if let Some(dispatcher) = dispatcher {
        let _ = dispatcher.join();
    }
    let mut state = lock(&inner.state);
    state.running = false;
}

/// 派发线程：把事件交给回调，并按路由给的截止时间醒来 `poll`。
fn dispatch(
    inner: Arc<Inner>,
    handler: EventHandler,
    owner: Option<Box<dyn CaptureLoop>>,
    rx: mpsc::Receiver<Message>,
) {
    let routed = owner.is_some();
    if let Some(owner) = owner {
        // 唤醒源已经在 `start_inner` 里装好，这里只把 owner 放进线程局部变量。
        crate::capture_loop::install_owner(owner);
    }
    loop {
        let deadline = if routed {
            let (due, stopped) = crate::capture_loop::poll(Instant::now());
            if stopped {
                break;
            }
            due
        } else {
            None
        };
        let message = match deadline {
            Some(due) => {
                let wait = due
                    .saturating_duration_since(Instant::now())
                    .max(std::time::Duration::from_millis(1));
                match rx.recv_timeout(wait) {
                    Ok(message) => Some(message),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            None => match rx.recv() {
                Ok(message) => Some(message),
                Err(_) => break,
            },
        };
        match message {
            Some(Message::Event(event)) => {
                // 内核设备是被动读取的：回调的返回值被忽略，本机照常处理这些事件。
                let _ = handler(&event);
            }
            Some(Message::Wake) => {}
            Some(Message::Stop) => break,
            Some(Message::CaptureReady) => crate::capture_loop::capture_available(true),
            Some(Message::CaptureFailed) => crate::capture_loop::capture_available(false),
            None => {}
        }
    }
    if routed {
        crate::capture_loop::clear();
    }
    let mut state = lock(&inner.state);
    state.running = false;
}

/// 读取线程：读内核设备，把事件派发出去。
fn read_devices(inner: Arc<Inner>, mut devices: Vec<Device>, tx: Sender<Message>, flag: Arc<AtomicBool>) {
    let started = Instant::now();
    // 设备已经打开：截获可用，界面不再把键鼠显示成「不可用」。
    let _ = tx.send(Message::CaptureReady);
    let mut buffer = vec![0u8; READ_BUFFER];
    // 位移是设备计数，转成逻辑像素（鼠标的 DPI 决定系数）。
    let mut motion = (0f64, 0f64);
    let mut wheel = (0f64, 0f64);
    let mut fds: Vec<libc::pollfd> = devices
        .iter()
        .map(|device| libc::pollfd {
            fd: device.file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    while !flag.load(Ordering::Relaxed) {
        // SAFETY: `fds` is a live slice of `pollfd` with one entry per open
        // device; the kernel only writes the `revents` fields.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 50) };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            tracing::warn!("读取输入设备失败：{error}");
            let _ = tx.send(Message::CaptureFailed);
            break;
        }
        for (index, device) in devices.iter_mut().enumerate() {
            if fds[index].revents == 0 {
                continue;
            }
            fds[index].revents = 0;
            for event in device.read(&mut buffer) {
                match event.kind {
                    evdev::EV_REL => {
                        let counts = f64::from(event.value);
                        match event.code {
                            evdev::REL_X => motion.0 += counts / device.counts_per_pixel,
                            evdev::REL_Y => motion.1 += counts / device.counts_per_pixel,
                            // `REL_WHEEL` 是旧式滚轮：一个单位就是一格。
                            // 高分辨率滚轮（1/120 格）另发 `REL_WHEEL_HI_RES`，
                            // 这里不读它，读旧式的那个就够了。
                            evdev::REL_WHEEL => wheel.1 += counts,
                            evdev::REL_HWHEEL => wheel.0 += counts,
                            _ => {}
                        }
                    }
                    evdev::EV_KEY => {
                        if let Some(captured) = key_event(&inner, event.code, event.value != 0, started)
                            && tx.send(Message::Event(Box::new(captured))).is_err()
                        {
                            // 派发线程已经退出：没人要这些事件了。
                            return;
                        }
                    }
                    evdev::EV_SYN => {
                        if event.code != evdev::SYN_REPORT {
                            continue;
                        }
                        if motion != (0.0, 0.0) {
                            let captured = motion_event(&inner, motion, started);
                            motion = (0.0, 0.0);
                            if let Some(captured) = captured
                                && tx.send(Message::Event(Box::new(captured))).is_err()
                            {
                                return;
                            }
                        }
                        if wheel != (0.0, 0.0) {
                            let captured = scroll_event(&inner, wheel, started);
                            wheel = (0.0, 0.0);
                            if let Some(captured) = captured
                                && tx.send(Message::Event(Box::new(captured))).is_err()
                            {
                                return;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// 一个按键或鼠标键。
fn key_event(inner: &Arc<Inner>, code: u16, pressed: bool, started: Instant) -> Option<CapturedEvent> {
    if (evdev::BTN_LEFT..=evdev::BTN_EXTRA + 2).contains(&code) {
        // 鼠标键。
        let button = match code {
            evdev::BTN_LEFT => MouseButton::Left,
            evdev::BTN_RIGHT => MouseButton::Right,
            evdev::BTN_MIDDLE => MouseButton::Middle,
            evdev::BTN_SIDE => MouseButton::Back,
            evdev::BTN_EXTRA => MouseButton::Forward,
            other => MouseButton::Other(other.saturating_sub(evdev::BTN_LEFT) as u8),
        };
        let cursor = current_cursor(inner);
        return Some(captured(
            InputEvent::Button { button, pressed },
            SourceKind::Mouse,
            started,
            cursor,
        ));
    }
    // 键盘：内核键码 → HID Usage。
    let usage = keycodes::hid(code)?;
    let cursor = current_cursor(inner);
    Some(captured(
        InputEvent::Key {
            code: KeyCode::keyboard(usage),
            pressed,
        },
        SourceKind::Keyboard,
        started,
        cursor,
    ))
}

/// 一次指针移动。
fn motion_event(inner: &Arc<Inner>, motion: (f64, f64), started: Instant) -> Option<CapturedEvent> {
    let size = desktop_size(inner);
    let (remote, park) = {
        let mut state = lock(&inner.state);
        if matches!(state.mode, CaptureMode::Remote { .. }) {
            // `Remote` 模式：本机指针停在停靠点，记住的位置也就是停靠点。
            (true, state.park)
        } else {
            state.cursor = clamp_to_desktop(Point::new(state.cursor.x + motion.0, state.cursor.y + motion.1), size);
            (false, None)
        }
    };
    let cursor = current_cursor(inner);
    if remote {
        // `Remote` 模式：本机指针不能漂。把指针挪回停靠点。
        if let Some(park) = park {
            let injector = lock(&inner.injector);
            if let Some(injector) = injector.as_ref() {
                let _ = injector.move_to(park, size);
            }
        }
    }
    Some(captured(
        InputEvent::PointerDelta {
            dx: motion.0,
            dy: motion.1,
        },
        SourceKind::Mouse,
        started,
        if remote { None } else { cursor },
    ))
}

/// 一次滚动。
fn scroll_event(inner: &Arc<Inner>, wheel: (f64, f64), started: Instant) -> Option<CapturedEvent> {
    let cursor = current_cursor(inner);
    Some(captured(
        InputEvent::Scroll {
            dx: wheel.0,
            dy: wheel.1,
            unit: ScrollUnit::Line,
            phase: ScrollPhase::None,
            wheel_steps: None,
        },
        SourceKind::Mouse,
        started,
        cursor,
    ))
}

/// 当前的光标位置：`Remote` 模式下是停靠点。
fn current_cursor(inner: &Arc<Inner>) -> Option<Point> {
    let state = lock(&inner.state);
    if matches!(state.mode, CaptureMode::Remote { .. }) {
        None
    } else {
        Some(state.cursor)
    }
}

/// 组装一个截获事件。
fn captured(event: InputEvent, source: SourceKind, started: Instant, cursor: Option<Point>) -> CapturedEvent {
    CapturedEvent {
        physical: true,
        event,
        source,
        time_us: started.elapsed().as_micros() as u64,
        cursor,
    }
}

/// 把坐标裁到桌面范围内。
fn clamp_to_desktop(point: Point, size: (u16, u16)) -> Point {
    Point::new(clamp_axis(point.x, size.0), clamp_axis(point.y, size.1))
}

/// 把一个坐标轴的值裁到 `[0, extent - 1]`。
fn clamp_axis(value: f64, extent: u16) -> f64 {
    let max = f64::from(extent.saturating_sub(1));
    value.round().clamp(0.0, max)
}
