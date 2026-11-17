//! macOS 实现：`CGGetOnlineDisplayList`、`CGDisplayBounds`、`CGDisplayScreenSize`、
//! `CGDisplayRegisterReconfigurationCallback`。
//!
//! - `bounds`：Quartz 全局坐标（point，主屏左上角为原点，y 向下）。
//! - 像素：当前显示模式的 backing store 像素（`CGDisplayModeGetPixelWidth`），Retina 通常是 point 的 2 倍；
//!   「更多空间」等缩放模式下是缩放后的帧缓冲像素，不是面板的原生像素。
//! - 物理尺寸：`CGDisplayScreenSize`（来自 EDID）。它不随旋转交换宽高，这里按 `bounds` 的方向校正；
//!   EDID 无效时系统按 72 dpi 伪造一个值，这种情况识别为「读不到」。
//! - 名字与虚拟显示器：CoreDisplay 的私有函数 `CoreDisplay_DisplayCreateInfoDictionary`（运行时 `dlsym`，
//!   不存在时降级），其中有 `DisplayProductName`、`kCGDisplayIsVirtualDevice`、`kCGDisplayIsAirPlay`。
//! - 镜像：镜像组里只保留主显示器（被镜像的那块），其余跳过。

mod arrangement;
mod info;
mod wallpaper;
mod watch;

use std::sync::Mutex;

use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayBounds, CGDisplayCopyDisplayMode, CGDisplayIsBuiltin, CGDisplayIsMain,
    CGDisplayMirrorsDisplay, CGDisplayMode, CGDisplayModelNumber, CGDisplayRotation, CGDisplayScreenSize,
    CGDisplaySerialNumber, CGDisplayVendorNumber, CGError, CGGetOnlineDisplayList,
};
use oink_foundation::display::DisplayInfo;
use oink_foundation::geom::Rect;

use crate::extent::{needs_swap, reference_extent};
use crate::{ChangeHandler, DisplayBackend, DisplayError};

pub(crate) fn backend() -> Result<Box<dyn DisplayBackend>, DisplayError> {
    Ok(Box::new(MacDisplays {
        watcher: Mutex::new(None),
    }))
}

struct MacDisplays {
    watcher: Mutex<Option<watch::Watcher>>,
}

impl DisplayBackend for MacDisplays {
    fn native_configuration(&self) -> Result<crate::arrangement::NativeConfiguration, DisplayError> {
        arrangement::configuration()
    }
    fn apply_native_origins(
        &self,
        expected: &crate::arrangement::NativeConfiguration,
        origins: &[crate::arrangement::NativeOrigin],
    ) -> Result<crate::arrangement::NativeConfiguration, DisplayError> {
        arrangement::apply(expected, origins)
    }
    fn displays(&self) -> Result<Vec<DisplayInfo>, DisplayError> {
        enumerate()
    }

    fn watch(&self, on_change: ChangeHandler) -> Result<(), DisplayError> {
        let mut w = self.watcher.lock().unwrap_or_else(|e| e.into_inner());
        match w.as_ref() {
            Some(existing) => existing.add_handler(on_change),
            None => *w = Some(watch::Watcher::start(on_change)?),
        }
        Ok(())
    }

    fn wallpapers(&self) -> Option<Vec<crate::wallpaper::DisplayWallpaper>> {
        wallpaper::read_all()
    }

    fn watch_wallpaper(&self, on_change: ChangeHandler) -> Result<(), DisplayError> {
        wallpaper::watch_spaces(on_change);
        Ok(())
    }
}

/// 在线显示器的 ID（含休眠中的显示器与镜像组的从属显示器）。
///
/// 用在线列表而不是活动列表：显示器休眠（锁屏后熄屏）时活动列表为空，本机会被当成「没有屏幕」，
/// 布局随之抖动；在线列表里休眠的显示器仍在，bounds 与显示模式照常可读。
pub(crate) fn online_display_ids() -> Result<Vec<CGDirectDisplayID>, DisplayError> {
    let mut count: u32 = 0;
    // SAFETY: 传空指针时只返回数量。
    let err = unsafe { CGGetOnlineDisplayList(0, std::ptr::null_mut(), &mut count) };
    check(err, "CGGetOnlineDisplayList")?;
    let mut ids = vec![0 as CGDirectDisplayID; count as usize + 4];
    let mut n: u32 = 0;
    // SAFETY: 缓冲区长度与传入的 max 一致。
    let err = unsafe { CGGetOnlineDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut n) };
    check(err, "CGGetOnlineDisplayList")?;
    ids.truncate(n as usize);
    Ok(ids)
}

fn check(err: CGError, what: &str) -> Result<(), DisplayError> {
    if err == CGError::Success {
        Ok(())
    } else {
        Err(DisplayError::Platform(format!("{what} 返回 {}", err.0)))
    }
}

fn enumerate() -> Result<Vec<DisplayInfo>, DisplayError> {
    let ids = online_display_ids()?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        // 镜像组：从属显示器（镜像了别人）跳过，镜像组视为一块屏幕。
        if CGDisplayMirrorsDisplay(id) != 0 {
            continue;
        }
        out.push(describe(id));
    }
    // 主屏排在最前，其余按 x、y 排序，输出稳定。
    out.sort_by(|a, b| {
        b.is_primary
            .cmp(&a.is_primary)
            .then(a.bounds.x.total_cmp(&b.bounds.x))
            .then(a.bounds.y.total_cmp(&b.bounds.y))
    });
    Ok(out)
}

fn describe(id: CGDirectDisplayID) -> DisplayInfo {
    let b = CGDisplayBounds(id);
    let bounds = Rect::new(b.origin.x, b.origin.y, b.size.width, b.size.height);
    let rotation_deg = normalize_rotation(CGDisplayRotation(id));

    let (mut pw, mut ph) = match CGDisplayCopyDisplayMode(id) {
        Some(mode) => (
            CGDisplayMode::pixel_width(Some(&mode)) as u32,
            CGDisplayMode::pixel_height(Some(&mode)) as u32,
        ),
        None => ((bounds.w) as u32, (bounds.h) as u32),
    };
    if needs_swap(f64::from(pw), f64::from(ph), &bounds) {
        std::mem::swap(&mut pw, &mut ph);
    }

    let s = CGDisplayScreenSize(id);
    let reference_extent = reference_extent(s.width * 0.001, s.height * 0.001, &bounds, pw, ph);

    let extra = info::display_info(id);
    let is_builtin = CGDisplayIsBuiltin(id);
    let vendor = CGDisplayVendorNumber(id);
    let model = CGDisplayModelNumber(id);
    let serial = CGDisplaySerialNumber(id);

    let key = info::display_uuid(id)
        .or_else(|| extra.uuid.clone())
        .unwrap_or_else(|| format!("cg:{vendor:04x}-{model:04x}-{serial:08x}"));
    let name = extra
        .product_name
        .clone()
        .map(|n| {
            if is_builtin && n == "Color LCD" {
                "Built-in Display".to_owned()
            } else {
                n
            }
        })
        .unwrap_or_else(|| {
            if is_builtin {
                "Built-in Display".to_owned()
            } else {
                format!("Display {id}")
            }
        });
    let is_virtual = extra.is_virtual.unwrap_or(false)
        || (!is_builtin && vendor == 0 && model == 0 && reference_extent.is_none())
        || looks_virtual(&name);

    DisplayInfo {
        key,
        name,
        bounds,
        pixel_width: pw,
        pixel_height: ph,
        reference_extent,
        rotation_deg,
        is_primary: CGDisplayIsMain(id),
        is_builtin,
        is_virtual,
    }
}

/// `CGDisplayRotation` 返回顺时针角度（double）；取最近的 90° 倍数。
fn normalize_rotation(deg: f64) -> u16 {
    let r = (deg / 90.0).round() as i64;
    (r.rem_euclid(4) * 90) as u16
}

/// 名字里带有明显虚拟屏特征（远程桌面、无头显示器软件）时判为虚拟显示器。
fn looks_virtual(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "virtual",
        "dummy",
        "headless",
        "vnc",
        "remote display",
        "parsec",
        "deskpad",
    ]
    .iter()
    .any(|k| n.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_is_normalized() {
        assert_eq!(normalize_rotation(0.0), 0);
        assert_eq!(normalize_rotation(90.0), 90);
        assert_eq!(normalize_rotation(270.0), 270);
        assert_eq!(normalize_rotation(-90.0), 270);
        assert_eq!(normalize_rotation(360.0), 0);
    }

    #[test]
    fn virtual_names_follow_supported_markers() {
        for name in [
            "Virtual Display",
            "BetterDisplay Dummy 16:9",
            "headless framebuffer",
            "VNC Display",
            "Remote Display 1",
            "Parsec Virtual Monitor",
            "DeskPad",
        ] {
            assert!(looks_virtual(name), "{name}");
        }
        for name in ["DELL U2720Q", "Studio Display", "Built-in Retina Display", "Monitor"] {
            assert!(!looks_virtual(name), "{name}");
        }
    }
}
