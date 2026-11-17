//! XRecord 截获。
//!
//! RECORD 需要两条连接：控制连接（建立 / 关闭 record context）与数据连接（读被记录的事件）。
//! 截获到的事件是原始协议数据（32 字节一个），这里按 `xproto` 的事件结构解析。
//!
//! 两条线程：
//! - 读取线程：读 RECORD 数据流，把事件翻译成 Oink 的事件；
//! - 派发线程：把事件交给回调；路由模式（[`start_loop`]）下这条线程还持有路由的 owner，
//!   按路由给的截止时间醒来 `poll`。
//!
//! X11 没有事件过滤钩子：截获回调的返回值被忽略，本机照常处理这些事件。
//! 自己注入的事件会被 RECORD 记下来，靠注入台账（`Pending`）过滤，避免回环。

use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use oink_foundation::Point;
use oink_foundation::input::{CaptureMode, CapturedEvent, InputEvent, KeyCode, SourceKind};

use x11rb::connection::Connection;
use x11rb::protocol::record::{self, ConnectionExt as _};
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::x11_utils::TryParse;

use crate::capture_loop::{CaptureLoop, Wake};
use crate::linux::{Inner, lock};
use crate::linux_keymap as keymap;
use crate::{EventHandler, InputError};

/// RECORD 回复的类别：服务器发来的协议数据。
const RECORD_FROM_SERVER: u8 = 0;

/// 等待路由截止时间时的最小等待：避免截止时间已过时空转。
const MIN_WAIT: Duration = Duration::from_millis(1);

/// 派发线程收到的一条消息。
pub(crate) enum Message {
    /// 截获到的物理事件。
    Event(Box<CapturedEvent>),
    /// 路由有活干：醒来重新 `poll`。
    Wake,
    /// 停止截获。
    Stop,
    /// 读取线程没能启用 RECORD：截获不可用。
    CaptureFailed,
    /// 读取线程已经启用 RECORD：截获可用。
    CaptureReady,
}

/// 截获到的原始事件。
enum Recorded {
    /// 按键。
    Key {
        /// X11 键码。
        keycode: u8,
        /// 是按下还是抬起。
        pressed: bool,
    },
    /// 鼠标按键。
    Button {
        /// X11 按钮号。
        button: u8,
        /// 是按下还是抬起。
        pressed: bool,
    },
    /// 指针移动。
    Motion {
        /// 根窗口横坐标。
        x: i16,
        /// 根窗口纵坐标。
        y: i16,
    },
    /// 不认识的协议数据：跳过。
    Skip,
}

/// 开始截获：建立 RECORD context、起读取线程与派发线程。
pub(crate) fn start(inner: &Arc<Inner>, handler: EventHandler) -> Result<(), InputError> {
    start_inner(inner, handler, None)
}

/// 开始截获，并把路由的 owner 交给派发线程（路由模式）。
///
/// 路由的 owner 只能在一条线程上用（`capture_loop` 用线程局部变量保存），
/// 因此派发线程既派发事件，也按路由给的截止时间醒来 `poll`。
pub(crate) fn start_loop(inner: &Arc<Inner>, owner: Box<dyn CaptureLoop>) -> Result<(), InputError> {
    start_inner(inner, Arc::new(crate::capture_loop::event), Some(owner))
}

/// 建立 RECORD context、起读取线程与派发线程。
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
    // 数据连接：RECORD 的数据必须从另一条连接读。
    let (data, _) =
        RustConnection::connect(None).map_err(|e| InputError::Platform(format!("连接 X 服务器失败：{e}")))?;
    let context = {
        let x11 = lock(&inner.x11);
        let context = x11
            .conn
            .generate_id()
            .map_err(|e| InputError::Platform(format!("分配 RECORD context 失败：{e}")))?;
        x11.conn
            .record_create_context(context, 0, &[record::CS::ALL_CLIENTS.into()], &[device_event_range()])
            .map_err(|e| InputError::Platform(format!("创建 RECORD context 失败：{e}")))?
            .check()
            .map_err(|e| InputError::Platform(format!("创建 RECORD context 失败：{e}")))?;
        context
    };
    // 键盘映射先读一次：第一个按键不用再等一次往返。
    {
        let mut x11 = lock(&inner.x11);
        let _ = x11.refresh_map();
    }
    let initial = current_root(inner);
    {
        let mut state = lock(&inner.state);
        state.running = true;
        state.pending.clear();
        state.last_root = initial;
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
    let reader = std::thread::Builder::new().name("oink-x11-record".to_string()).spawn({
        let inner = Arc::clone(inner);
        let tx = tx.clone();
        move || read_records(inner, data, context, tx)
    });
    let reader = match reader {
        Ok(reader) => reader,
        Err(error) => {
            let x11 = lock(&inner.x11);
            let _ = x11.conn.record_disable_context(context);
            let _ = x11.conn.sync();
            let mut state = lock(&inner.state);
            state.running = false;
            return Err(InputError::Platform(format!("启动截获线程失败：{error}")));
        }
    };
    let dispatcher = std::thread::Builder::new()
        .name("oink-x11-dispatch".to_string())
        .spawn({
            let inner = Arc::clone(inner);
            move || dispatch(inner, handler, owner, rx)
        });
    let dispatcher = match dispatcher {
        Ok(dispatcher) => dispatcher,
        Err(error) => {
            // 派发线程起不来：关掉 context、收掉读取线程，别在服务器上留一个空转的记录。
            let x11 = lock(&inner.x11);
            let _ = x11.conn.record_disable_context(context);
            let _ = x11.conn.sync();
            let _ = reader.join();
            let mut state = lock(&inner.state);
            state.running = false;
            return Err(InputError::Platform(format!("启动派发线程失败：{error}")));
        }
    };
    let mut slot = lock(&inner.capture);
    slot.context = Some(context);
    slot.reader = Some(reader);
    slot.dispatcher = Some(dispatcher);
    slot.stop = Some(tx);
    Ok(())
}

/// 停止截获：关掉 record context、join 两条线程。
pub(crate) fn stop(inner: &Inner) {
    let (context, reader, dispatcher, stop) = {
        let mut slot = lock(&inner.capture);
        (
            slot.context.take(),
            slot.reader.take(),
            slot.dispatcher.take(),
            slot.stop.take(),
        )
    };
    if let Some(context) = context {
        let x11 = lock(&inner.x11);
        // 关掉 context 之后，数据连接会收到「数据结束」，读取线程自己退出。
        let _ = x11.conn.record_disable_context(context);
        let _ = x11.conn.sync();
    }
    // 派发线程在等消息：叫醒它，让它走完这一轮再退出。
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
    state.pending.clear();
}

/// 读取线程：读 RECORD 数据流，把事件翻译成 Oink 的事件。
fn read_records(inner: Arc<Inner>, data: RustConnection, context: u32, tx: mpsc::Sender<Message>) {
    let started = Instant::now();
    let stream = match data.record_enable_context(context) {
        Ok(stream) => stream,
        Err(error) => {
            tracing::error!("XRecord 启用失败：{error}");
            let _ = tx.send(Message::CaptureFailed);
            return;
        }
    };
    // RECORD 已经启用：截获可用，界面不再把键鼠显示成「不可用」。
    let _ = tx.send(Message::CaptureReady);
    for reply in stream {
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                tracing::warn!("XRecord 读取失败：{error}");
                let _ = tx.send(Message::CaptureFailed);
                break;
            }
        };
        if reply.client_swapped || reply.category != RECORD_FROM_SERVER {
            continue;
        }
        handle_data(&inner, &reply.data, started, &tx);
    }
}

/// 派发线程：把事件交给回调；路由模式下还按路由给的截止时间醒来 `poll`。
fn dispatch(
    inner: Arc<Inner>,
    handler: EventHandler,
    owner: Option<Box<dyn CaptureLoop>>,
    rx: mpsc::Receiver<Message>,
) {
    let routed = owner.is_some();
    if let Some(owner) = owner {
        // 唤醒源已经在 [`start_inner`] 里装好，这里只把 owner 放进线程局部变量。
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
                let wait = due.saturating_duration_since(Instant::now()).max(MIN_WAIT);
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
                // X11 没有事件过滤钩子：回调的返回值被忽略，本机照常处理这些事件。
                let _ = handler(&event);
            }
            Some(Message::Wake) => {}
            Some(Message::Stop) => break,
            // 截获可用性由读取线程观察：RECORD 启用失败时界面要看到「不可用」。
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

/// 处理一段 RECORD 数据：里面是若干个 32 字节的原始事件。
fn handle_data(inner: &Arc<Inner>, data: &[u8], started: Instant, tx: &mpsc::Sender<Message>) {
    let mut rest = data;
    while !rest.is_empty() {
        let Some((event, remaining)) = parse_event(rest) else {
            break;
        };
        rest = remaining;
        let captured = match event {
            Recorded::Key { keycode, pressed } => key_event(inner, keycode, pressed, started),
            Recorded::Button { button, pressed } => button_event(inner, button, pressed, started),
            Recorded::Motion { x, y } => motion_event(inner, x, y, started),
            Recorded::Skip => None,
        };
        if let Some(captured) = captured
            && tx.send(Message::Event(Box::new(captured))).is_err()
        {
            // 派发线程已经退出：没人要这些事件了。
            return;
        }
    }
}

/// 解析一个原始事件，返回事件与剩下的数据。
fn parse_event(data: &[u8]) -> Option<(Recorded, &[u8])> {
    // 设备事件可能带「由设备产生」的高位。
    let code = *data.first()? & 0x7f;
    match code {
        xproto::KEY_PRESS_EVENT => {
            let (event, rest) = xproto::KeyPressEvent::try_parse(data).ok()?;
            Some((
                Recorded::Key {
                    keycode: event.detail,
                    pressed: true,
                },
                rest,
            ))
        }
        xproto::KEY_RELEASE_EVENT => {
            let (event, rest) = xproto::KeyReleaseEvent::try_parse(data).ok()?;
            Some((
                Recorded::Key {
                    keycode: event.detail,
                    pressed: false,
                },
                rest,
            ))
        }
        xproto::BUTTON_PRESS_EVENT => {
            let (event, rest) = xproto::ButtonPressEvent::try_parse(data).ok()?;
            Some((
                Recorded::Button {
                    button: event.detail,
                    pressed: true,
                },
                rest,
            ))
        }
        xproto::BUTTON_RELEASE_EVENT => {
            let (event, rest) = xproto::ButtonReleaseEvent::try_parse(data).ok()?;
            Some((
                Recorded::Button {
                    button: event.detail,
                    pressed: false,
                },
                rest,
            ))
        }
        xproto::MOTION_NOTIFY_EVENT => {
            let (event, rest) = xproto::MotionNotifyEvent::try_parse(data).ok()?;
            Some((
                Recorded::Motion {
                    x: event.root_x,
                    y: event.root_y,
                },
                rest,
            ))
        }
        _ => {
            // 其它协议数据（错误、扩展事件）：固定 32 字节，跳过。
            if data.len() < 32 {
                None
            } else {
                Some((Recorded::Skip, &data[32..]))
            }
        }
    }
}

/// 一个按键。
fn key_event(inner: &Inner, keycode: u8, pressed: bool, started: Instant) -> Option<CapturedEvent> {
    if lock(&inner.state).take_pending_key(keycode, pressed) {
        // 自己注入的按键。
        return None;
    }
    let code = hid_for_keycode(inner, keycode)?;
    Some(captured(
        InputEvent::Key { code, pressed },
        SourceKind::Keyboard,
        started,
        None,
    ))
}

/// 一个鼠标按键。
fn button_event(inner: &Inner, button: u8, pressed: bool, started: Instant) -> Option<CapturedEvent> {
    if lock(&inner.state).take_pending_button(button, pressed) {
        // 自己注入的按键。
        return None;
    }
    let remote = matches!(lock(&inner.state).mode, CaptureMode::Remote { .. });
    let cursor = if remote { None } else { current_pointer(inner) };
    Some(captured(
        InputEvent::Button {
            button: keymap::button_from_x11(button),
            pressed,
        },
        SourceKind::Mouse,
        started,
        cursor,
    ))
}

/// 一次指针移动。
fn motion_event(inner: &Inner, x: i16, y: i16, started: Instant) -> Option<CapturedEvent> {
    let self_injected = lock(&inner.state).take_pending_motion(x, y);
    let (mode, park, previous) = {
        let mut state = lock(&inner.state);
        let previous = state.last_root;
        let remote = matches!(state.mode, CaptureMode::Remote { .. });
        if self_injected {
            // 自己写的位置：指针确实在那里，但这不是用户的移动。
            state.last_root = Some((x, y));
        } else if !remote {
            state.last_root = Some((x, y));
        }
        (state.mode, state.park, previous)
    };
    if self_injected {
        return None;
    }
    let previous = previous?;
    // 第一次看到位置：还没有位移可言。
    let dx = f64::from(x) - f64::from(previous.0);
    let dy = f64::from(y) - f64::from(previous.1);
    let remote = matches!(mode, CaptureMode::Remote { .. });
    if remote {
        // Remote 模式：本机指针不能漂。XRecord 是被动记录，服务器已经把指针挪过去了，
        // 这里用 XTEST 把它挪回停靠点（挪回去的动作登记在台账里，不会再被当成用户移动）。
        if let Some((park_x, park_y)) = park
            && (park_x, park_y) != (x, y)
            && let Err(error) = super::warp_pointer(inner, park_x, park_y)
        {
            tracing::warn!("挪回停靠点失败：{error}");
        }
    }
    let cursor = if remote {
        None
    } else {
        Some(Point::new(f64::from(x), f64::from(y)))
    };
    Some(captured(
        InputEvent::PointerDelta { dx, dy },
        SourceKind::Mouse,
        started,
        cursor,
    ))
}

/// 组装一个截获事件。
fn captured(event: InputEvent, source: SourceKind, started: Instant, cursor: Option<Point>) -> CapturedEvent {
    CapturedEvent {
        physical: true,
        event,
        source,
        // 截获开始以来的单调微秒。
        time_us: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
        cursor,
    }
}

/// 键码 → HID Usage（键码 → keysym → HID）。
fn hid_for_keycode(inner: &Inner, keycode: u8) -> Option<KeyCode> {
    let mut x11 = lock(&inner.x11);
    if x11.map.is_empty() {
        let _ = x11.refresh_map();
    }
    let keysym = x11.map.keysym(keycode)?;
    keymap::hid_from_keysym(keysym)
}

/// 当前指针位置（根窗口坐标）。
fn current_root(inner: &Inner) -> Option<(i16, i16)> {
    lock(&inner.state).last_root
}

/// 当前指针位置（本机全局坐标）。
fn current_pointer(inner: &Inner) -> Option<Point> {
    if let Some((x, y)) = current_root(inner) {
        return Some(Point::new(f64::from(x), f64::from(y)));
    }
    // 没有观测过位置：问一次服务器。
    let x11 = lock(&inner.x11);
    let root = x11.root();
    let reply = x11.conn.query_pointer(root).ok()?.reply().ok()?;
    Some(Point::new(f64::from(reply.root_x), f64::from(reply.root_y)))
}

/// RECORD 的截获范围：核心协议里按键到移动通知之间的设备事件。
fn device_event_range() -> record::Range {
    let empty = record::Range8 { first: 0, last: 0 };
    let empty_ext = record::ExtRange {
        major: empty,
        minor: record::Range16 { first: 0, last: 0 },
    };
    record::Range {
        core_requests: empty,
        core_replies: empty,
        ext_requests: empty_ext,
        ext_replies: empty_ext,
        delivered_events: empty,
        device_events: record::Range8 {
            first: xproto::KEY_PRESS_EVENT,
            last: xproto::MOTION_NOTIFY_EVENT,
        },
        errors: empty,
        client_started: false,
        client_died: false,
    }
}

#[cfg(all(test, debug_assertions))]
// 无 libtest 的构建会去掉 `#[test]` 的函数体，这些辅助函数因此没人调用。
#[allow(dead_code)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use crate::capture_loop::{CaptureLoop, Wake};
    use crate::{CapturedEvent, Disposition};

    /// 记录 owner 收到的截获可用性与唤醒源。
    struct Recorder {
        /// owner 收到的可用性。
        available: Arc<AtomicBool>,
        /// owner 收到过通知。
        reported: Arc<AtomicBool>,
        /// owner 拿到了唤醒源。
        wake: Arc<AtomicBool>,
    }

    impl CaptureLoop for Recorder {
        fn set_wake(&mut self, _wake: Wake) {
            self.wake.store(true, Ordering::Release);
        }

        fn event(&mut self, _event: &CapturedEvent) -> Disposition {
            Disposition::Pass
        }

        fn capture_available(&mut self, available: bool) {
            self.available.store(available, Ordering::Release);
            self.reported.store(true, Ordering::Release);
        }

        fn poll(&mut self, _now: Instant) -> Option<Instant> {
            None
        }

        fn stopped(&self) -> bool {
            false
        }
    }

    /// 截获可用时 owner 必须知道：界面靠这个把键鼠显示成「可以控制」。
    ///
    /// 需要真的 X 服务器（RECORD 扩展）：容器里的测试脚本会起一个 Xvfb。
    #[test]
    fn the_capture_reports_its_availability_to_the_owner() {
        if std::env::var_os("DISPLAY").is_none() {
            // 没有 X 会话就没有截获。
            return;
        }
        let available = Arc::new(AtomicBool::new(false));
        let reported = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(AtomicBool::new(false));
        let Ok(backend) = crate::linux::backend() else {
            // 服务器没有 RECORD 扩展：没有截获可测。
            return;
        };
        let owner = Box::new(Recorder {
            available: Arc::clone(&available),
            reported: Arc::clone(&reported),
            wake: Arc::clone(&wake),
        });
        backend.start_loop(owner).expect("截获可以启动");
        // 唤醒源必须在 start_loop 返回前就装好：路由线程紧接着发出的第一条
        // 命令会立刻用它，那时候截获线程还没来得及跑。
        assert!(wake.load(Ordering::Acquire), "唤醒源没有在截获启动时装好");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !reported.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        backend.stop();
        assert!(reported.load(Ordering::Acquire), "截获没有把可用性告诉 owner");
        assert!(available.load(Ordering::Acquire), "截获应该是可用的");
    }
}
