//! 当前选择的隐私提示。
//!
//! KDE 系的剪贴板管理器读 `x-kde-passwordManagerHint`：内容的所有者用这个目标
//! 回答"这段内容是秘密"。本设备写入 `Concealed` 内容时也用同一个目标
//! （arboard 的 `exclude_from_history`），所以读回它就能知道别人复制的内容是不是
//! 秘密，并把这份隐私级别一起公布给对端。
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, EventMask, WindowClass};
use x11rb::rust_connection::RustConnection;

/// KDE 约定的隐私提示目标。
pub(super) const HINT: &str = "x-kde-passwordManagerHint";

/// 所有者最多用这么久回答；超时就当作没有提示。
const TIMEOUT: Duration = Duration::from_millis(250);

/// 当前选择是否被它的所有者标记为秘密。
pub(super) fn concealed() -> bool {
    let Ok((connection, screen)) = RustConnection::connect(None) else {
        return false;
    };
    let Some(root) = connection.setup().roots.get(screen).map(|root| root.root) else {
        return false;
    };
    let Some(clipboard) = atom(&connection, b"CLIPBOARD") else {
        return false;
    };
    let Some(hint) = atom(&connection, HINT.as_bytes()) else {
        return false;
    };
    let owner = match connection.get_selection_owner(clipboard) {
        Ok(cookie) => match cookie.reply() {
            Ok(reply) => reply.owner,
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    if owner == x11rb::NONE {
        return false;
    }
    // 一个只用来接收转换结果的小窗口：所有者把提示写进它的属性。
    let window = match connection.generate_id() {
        Ok(id) => id,
        Err(_) => return false,
    };
    let created = connection.create_window(
        0,
        window,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        x11rb::COPY_FROM_PARENT,
        &x11rb::protocol::xproto::CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    );
    if !accepted(created) {
        return false;
    }
    // 属性必须是一个真实的原子：所有者把提示写进这个属性。
    let Some(property) = atom(&connection, b"OINK_CLIPBOARD_PRIVACY") else {
        let _ = connection.destroy_window(window);
        return false;
    };
    let converted = connection.convert_selection(window, clipboard, hint, property, x11rb::CURRENT_TIME);
    if !accepted(converted) {
        let _ = connection.destroy_window(window);
        return false;
    }
    let answer = answer(&connection, window);
    let _ = connection.destroy_window(window);
    answer
}

/// 原子：名字已知，返回它的编号。
fn atom(connection: &RustConnection, name: &[u8]) -> Option<u32> {
    match connection.intern_atom(false, name) {
        Ok(cookie) => match cookie.reply() {
            Ok(reply) => Some(reply.atom),
            Err(_) => None,
        },
        Err(_) => None,
    }
}

/// 请求被服务器接受。
fn accepted(cookie: Result<x11rb::cookie::VoidCookie<'_, RustConnection>, x11rb::errors::ConnectionError>) -> bool {
    match cookie {
        Ok(cookie) => cookie.check().is_ok(),
        Err(_) => false,
    }
}

/// 等待所有者回答：它把提示写进属性才算它标记了秘密。
fn answer(connection: &RustConnection, window: u32) -> bool {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match connection.poll_for_event() {
            Ok(Some(event)) => {
                if let x11rb::protocol::Event::SelectionNotify(notify) = event {
                    if notify.requestor != window {
                        continue;
                    }
                    if notify.property == x11rb::NONE {
                        // 所有者不认识这个目标：这段内容不是秘密。
                        return false;
                    }
                    return value(connection, window, notify.property);
                }
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(_) => return false,
        }
    }
}

/// 读属性上的值：任何非空内容都表示"秘密"。
fn value(connection: &RustConnection, window: u32, property: u32) -> bool {
    let reply = match connection.get_property(true, window, property, AtomEnum::ANY, 0, 1024) {
        Ok(cookie) => match cookie.reply() {
            Ok(reply) => reply,
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    !reply.value.is_empty()
}
