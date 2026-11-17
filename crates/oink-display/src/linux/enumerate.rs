//! 用 XRandR 枚举当前会话里的显示器。
//!
//! 走的是 RandR 1.2 的老路子（`RRGetScreenResourcesCurrent` → `RRGetOutputInfo` →
//! `RRGetCrtcInfo`）：每个 X 服务器都有，XWayland 也一样。

use oink_foundation::display::DisplayInfo;
use oink_foundation::geom::Rect;
use x11rb::connection::Connection as _;
use x11rb::errors::{ConnectionError, ReplyError};
use x11rb::protocol::randr;
use x11rb::protocol::xproto;
use x11rb::rust_connection::RustConnection;

use crate::DisplayError;

use super::geometry::{self, OutputGeometry};

/// 连上 X 服务器。
pub(crate) fn connect() -> Result<(RustConnection, usize), DisplayError> {
    RustConnection::connect(None).map_err(|error| DisplayError::Platform(format!("连接 X 服务器失败：{error}")))
}

/// 确认服务器有 RandR 扩展，而且至少是 1.2（枚举要用 `RRGetScreenResourcesCurrent`）。
pub(crate) fn ensure_randr(connection: &RustConnection) -> Result<(), DisplayError> {
    let version = randr::query_version(connection, 1, 5)
        .map_err(connection_error)?
        .reply()
        .map_err(reply_error)?;
    if version.major_version > 1 || (version.major_version == 1 && version.minor_version >= 2) {
        Ok(())
    } else {
        Err(DisplayError::Unsupported)
    }
}

/// 当前会话里的显示器。
pub(crate) fn displays(connection: &RustConnection, screen: usize) -> Result<Vec<DisplayInfo>, DisplayError> {
    let setup = connection.setup();
    let root = setup.roots[screen].root;
    let root_size = (
        u32::from(setup.roots[screen].width_in_pixels),
        u32::from(setup.roots[screen].height_in_pixels),
    );
    let resources = randr::get_screen_resources_current(connection, root)
        .map_err(connection_error)?
        .reply()
        .map_err(reply_error)?;
    let server_primary = primary_output(connection, root);
    let mut outputs: Vec<(u32, OutputGeometry)> = Vec::new();
    for output in &resources.outputs {
        let info = randr::get_output_info(connection, *output, 0)
            .map_err(connection_error)?
            .reply()
            .map_err(reply_error)?;
        // 没插上的输出、以及没接在 CRTC 上的输出都不算显示器。
        if u8::from(info.connection) != u8::from(randr::Connection::CONNECTED) || info.crtc == 0 {
            continue;
        }
        let crtc = randr::get_crtc_info(connection, info.crtc, 0)
            .map_err(connection_error)?
            .reply()
            .map_err(reply_error)?;
        if u8::from(crtc.status) != u8::from(randr::SetConfig::SUCCESS) {
            continue;
        }
        let entry = geometry_of(*output, &info, &crtc, &resources);
        match outputs.iter_mut().find(|(crtc, _)| *crtc == info.crtc) {
            // 同一个 CRTC 上有多个输出（镜像）：只留一个，主输出优先。
            Some(slot) => {
                if server_primary == Some(entry.output_id) && server_primary != Some(slot.1.output_id) {
                    slot.1 = entry;
                }
            }
            None => outputs.push((info.crtc, entry)),
        }
    }
    let geometries: Vec<OutputGeometry> = outputs.into_iter().map(|(_, entry)| entry).collect();
    // XWayland 报告的 CRTC 占位可能是缩放后的逻辑尺寸，指针与注入都在根窗口
    // 像素空间里：显示器的几何要换算过去，否则指针会被限制在一小块区域里。
    let scale = geometry::RootScale::between(root_size, geometry::crtc_extent(&geometries));
    let geometries: Vec<OutputGeometry> = geometries.iter().map(|entry| entry.scaled(scale)).collect();
    let primary = geometry::select_primary(&geometries, server_primary);
    Ok(geometries
        .iter()
        .map(|entry| entry.display_info(Some(entry.output_id) == primary))
        .collect())
}

/// 一个输出对应的几何信息。
fn geometry_of(
    output: u32,
    info: &randr::GetOutputInfoReply,
    crtc: &randr::GetCrtcInfoReply,
    resources: &randr::GetScreenResourcesCurrentReply,
) -> OutputGeometry {
    let (mode_width, mode_height) = match resources.modes.iter().find(|mode| mode.id == crtc.mode) {
        Some(mode) => (u32::from(mode.width), u32::from(mode.height)),
        // 模式表里没有这个模式（少见）：退回 CRTC 的扫描输出尺寸。
        None => (u32::from(crtc.width), u32::from(crtc.height)),
    };
    OutputGeometry {
        output_id: output,
        connector: connector_name(&info.name),
        // CRTC 报的是扫描输出的尺寸，旋转过的屏在这里已经是转过来的占位。
        bounds: Rect::new(
            f64::from(crtc.x),
            f64::from(crtc.y),
            f64::from(crtc.width),
            f64::from(crtc.height),
        ),
        mode_width,
        mode_height,
        rotation_deg: geometry::rotation_deg(u16::from(crtc.rotation)),
        mm_width: info.mm_width,
        mm_height: info.mm_height,
    }
}

/// 输出的连接名（`DP-1`…）。RandR 给的是以 NUL 结尾的字节串。
fn connector_name(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).trim_end_matches('\0').trim().to_string()
}

/// 服务器报告的主输出。
///
/// 这个请求是 RandR 1.3 才有的，更老的服务器会拒绝它，按「没有主输出」处理。
fn primary_output(connection: &RustConnection, root: xproto::Window) -> Option<u32> {
    randr::get_output_primary(connection, root)
        .ok()?
        .reply()
        .ok()
        .map(|reply| reply.output)
}

/// 连接层出错：服务器没有这个扩展就是「不支持」，其余算平台错误。
fn connection_error(error: ConnectionError) -> DisplayError {
    match error {
        ConnectionError::UnsupportedExtension => DisplayError::Unsupported,
        other => DisplayError::Platform(format!("X 请求失败：{other}")),
    }
}

/// 服务器拒绝了请求。
fn reply_error(error: ReplyError) -> DisplayError {
    DisplayError::Platform(format!("X 服务器拒绝了请求：{error}"))
}
