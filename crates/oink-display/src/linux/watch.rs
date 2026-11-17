//! 监听显示器配置变化。
//!
//! RandR 在根窗口上发 `RRScreenChangeNotify`（屏幕尺寸变了）和 `RROutputChange`、
//! `RRCrtcChange`（输出或 CRTC 变了）。一次物理变化会带好几个事件，所以先合并
//! 再读一次配置，变了才通知。

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use x11rb::connection::Connection as _;
use x11rb::protocol::randr;

use crate::{ChangeHandler, DisplayError, DisplayInfo};

use super::{Shared, enumerate, lock};

/// 注册的变化回调（可以注册多个）。
pub(crate) type Handlers = Arc<Mutex<Vec<ChangeHandler>>>;

/// 一次物理变化会带来好几个 RandR 事件，先合并再通知。
const DEBOUNCE: Duration = Duration::from_millis(200);

/// 监听线程。
#[derive(Debug)]
pub(crate) struct Watcher {
    /// 线程句柄。X 连接断开时线程自己结束。
    _thread: thread::JoinHandle<()>,
}

impl Watcher {
    /// 启动监听线程。
    pub(crate) fn spawn(handlers: Handlers, connection: Shared, screen: usize) -> Result<Self, DisplayError> {
        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        let handle = thread::Builder::new()
            .name("oink-display-watch".into())
            .spawn(move || run(handlers, connection, screen, tx))
            .map_err(|error| DisplayError::Platform(format!("启动监听线程失败：{error}")))?;
        match rx.recv() {
            Ok(Ok(())) => Ok(Self { _thread: handle }),
            Ok(Err(message)) => {
                let _ = handle.join();
                Err(DisplayError::Platform(message))
            }
            Err(_) => {
                let _ = handle.join();
                Err(DisplayError::Platform("监听线程没有启动".into()))
            }
        }
    }
}

/// 监听线程的主体。
fn run(handlers: Handlers, connection: Shared, screen: usize, tx: mpsc::Sender<Result<(), String>>) {
    // 事件用单独的连接收，免得和枚举抢同一个连接。
    let (events, event_screen) = match enumerate::connect() {
        Ok(pair) => pair,
        Err(error) => {
            let _ = tx.send(Err(format!("{error}")));
            return;
        }
    };
    let root = events.setup().roots[event_screen].root;
    let mask = randr::NotifyMask::from(
        u16::from(randr::NotifyMask::SCREEN_CHANGE)
            | u16::from(randr::NotifyMask::CRTC_CHANGE)
            | u16::from(randr::NotifyMask::OUTPUT_CHANGE),
    );
    let selected = randr::select_input(&events, root, mask);
    let subscribed = match selected {
        Ok(cookie) => cookie.check().map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    if let Err(message) = subscribed {
        let _ = tx.send(Err(format!("订阅 RandR 事件失败：{message}")));
        return;
    }
    // 订阅之后的配置就是基线，之后有变化才通知。
    let mut previous = snapshot(&connection, screen);
    if tx.send(Ok(())).is_err() {
        return;
    }
    loop {
        let event = match events.wait_for_event() {
            Ok(event) => event,
            Err(error) => {
                tracing::debug!("RandR 事件监听结束：{error}");
                break;
            }
        };
        if !is_randr_event(&event) {
            continue;
        }
        // 一次变化会带来好几个事件，合并之后再读配置。
        thread::sleep(DEBOUNCE);
        while events.poll_for_event().ok().flatten().is_some() {}
        let current = snapshot(&connection, screen);
        if let Some(current) = current
            && previous.as_ref() != Some(&current)
        {
            previous = Some(current);
            fire(&handlers);
        }
    }
}

/// RandR 的事件：屏幕尺寸变了，或者输出、CRTC 变了。
fn is_randr_event(event: &x11rb::protocol::Event) -> bool {
    matches!(
        event,
        x11rb::protocol::Event::RandrScreenChangeNotify(_) | x11rb::protocol::Event::RandrNotify(_)
    )
}

/// 读一次当前配置。读不到就按「不知道」处理，不通知。
fn snapshot(connection: &Shared, screen: usize) -> Option<Vec<DisplayInfo>> {
    let connection = lock(connection);
    match enumerate::displays(&connection, screen) {
        Ok(displays) => Some(displays),
        Err(error) => {
            tracing::warn!("读取显示器配置失败：{error}");
            None
        }
    }
}

/// 通知所有回调。
fn fire(handlers: &Handlers) {
    // 回调里可能再注册回调，先把列表复制出来再逐个调用。
    let handlers: Vec<ChangeHandler> = lock(handlers).clone();
    for handler in handlers {
        handler();
    }
}
