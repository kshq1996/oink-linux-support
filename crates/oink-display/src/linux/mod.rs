//! Linux（X11 / XRandR）实现：枚举显示器、监听配置变化。
//!
//! - 坐标：根窗口坐标系（X11 的全局坐标），原点在根窗口左上角，y 向下，单位是像素。
//! - 名字：RandR 输出的连接名（`DP-1`、`HDMI-1`…）。重新插拔同一个接口后仍然是这个名字，
//!   所以它同时就是稳定标识。
//! - 几何：来自驱动输出的 CRTC（`RRGetCrtcInfo`）。CRTC 报的是**扫描输出**的尺寸，
//!   旋转过的屏在这里已经是转过来的占位；模式尺寸按旋转摆好，就是像素数。
//! - 主显示器：优先用 RandR 1.3 的 `RRGetOutputPrimary`；服务器没有这个请求时，
//!   退回「根窗口原点落在哪个输出上」。
//! - 内置屏、虚拟屏：RandR 不提供这两个信息，一律 `false`。
//! - 面板物理尺寸：来自输出的毫米字段（EDID）；没有 EDID 时服务器会按 96dpi 伪造，
//!   这种尺寸按「读不到」处理。

mod enumerate;
mod geometry;
mod watch;

use std::sync::{Arc, Mutex, MutexGuard};

use x11rb::rust_connection::RustConnection;

use crate::{ChangeHandler, DisplayBackend, DisplayError, DisplayInfo};

/// 后端与监听线程共享的 X11 连接。
pub(crate) type Shared = Arc<Mutex<RustConnection>>;

/// 取锁。锁中毒时照常取用：监听线程 panic 不应该让整个后端失效。
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 创建本机（Linux / X11）的实现。
pub(crate) fn backend() -> Result<Box<dyn DisplayBackend>, DisplayError> {
    let (connection, screen) = enumerate::connect()?;
    enumerate::ensure_randr(&connection)?;
    Ok(Box::new(LinuxDisplays {
        connection: Arc::new(Mutex::new(connection)),
        screen,
        handlers: watch::Handlers::default(),
        watcher: Mutex::new(None),
    }))
}

/// Linux（X11）显示器后端。
struct LinuxDisplays {
    connection: Shared,
    screen: usize,
    handlers: watch::Handlers,
    watcher: Mutex<Option<watch::Watcher>>,
}

impl DisplayBackend for LinuxDisplays {
    fn displays(&self) -> Result<Vec<DisplayInfo>, DisplayError> {
        let connection = lock(&self.connection);
        enumerate::displays(&connection, self.screen)
    }

    fn watch(&self, on_change: ChangeHandler) -> Result<(), DisplayError> {
        lock(&self.handlers).push(on_change);
        let mut watcher = lock(&self.watcher);
        if watcher.is_none() {
            *watcher = Some(watch::Watcher::spawn(
                self.handlers.clone(),
                self.connection.clone(),
                self.screen,
            )?);
        }
        Ok(())
    }
}
