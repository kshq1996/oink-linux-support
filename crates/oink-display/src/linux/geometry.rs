//! 纯数据的几何换算：把 RandR 读到的一个输出变成 [`DisplayInfo`]。
//!
//! 这一层只处理普通数值，不碰协议结构体，因此可以在没有 X 服务器的情况下测试。

use oink_foundation::display::DisplayInfo;
use oink_foundation::geom::{Point, Rect, Size};

use crate::extent;

/// RandR 的旋转是位掩码（`randrproto.h`）：0x1 = 0°，0x2 = 90°，0x4 = 180°，0x8 = 270°。
const ROTATE_90: u16 = 1 << 1;
const ROTATE_180: u16 = 1 << 2;
const ROTATE_270: u16 = 1 << 3;

/// X 服务器在没有 EDID 时按 96dpi 伪造面板尺寸。
const FABRICATED_DPI: f64 = 96.0;
/// 一英寸的毫米数。
const MM_PER_INCH: f64 = 25.4;

/// 一个输出（显示器）的几何描述。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OutputGeometry {
    /// 输出的 XID：重新插拔后会变，只在连接名缺失时用来兜底。
    pub(crate) output_id: u32,
    /// 连接名（`DP-1`、`HDMI-1`…）：重新插拔同一个接口后仍然是这个名字。
    pub(crate) connector: String,
    /// 输出在根窗口坐标系里的矩形（已含旋转后的实际占位）。
    pub(crate) bounds: Rect,
    /// 当前模式的像素宽高（未按旋转交换）。
    pub(crate) mode_width: u32,
    /// 当前模式的像素宽高（未按旋转交换）。
    pub(crate) mode_height: u32,
    /// 顺时针旋转角度：0 / 90 / 180 / 270。
    pub(crate) rotation_deg: u16,
    /// 面板物理尺寸（毫米），来自 EDID。
    pub(crate) mm_width: u32,
    /// 面板物理尺寸（毫米），来自 EDID。
    pub(crate) mm_height: u32,
}

/// X11 根窗口与 RandR 扫描输出之间的比例。
///
/// XWayland 在桌面被缩放时按**逻辑尺寸**报告输出（CRTC 1280x800），而 X11 客户端
/// 活在**根窗口的像素空间**里（1920x1200）：指针的坐标、截获与注入都在根窗口空间。
/// 显示器的几何必须换算到根窗口空间，否则指针会被限制在逻辑尺寸那一小块区域里。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RootScale {
    /// 横向比例。
    pub(crate) x: f64,
    /// 纵向比例。
    pub(crate) y: f64,
}

impl RootScale {
    /// 根窗口尺寸与 CRTC 占位尺寸之间的比例。
    ///
    /// 两个方向的比例不一致时按 1 处理：那是输出没有铺满根窗口，不是缩放。
    pub(crate) fn between(root: (u32, u32), crtcs: (u32, u32)) -> Self {
        if crtcs.0 == 0 || crtcs.1 == 0 {
            return Self { x: 1.0, y: 1.0 };
        }
        let x = f64::from(root.0) / f64::from(crtcs.0);
        let y = f64::from(root.1) / f64::from(crtcs.1);
        if (x - y).abs() <= 0.01 * x.max(y) {
            Self { x, y }
        } else {
            Self { x: 1.0, y: 1.0 }
        }
    }

    /// 是不是 1:1（没有缩放）。
    pub(crate) fn is_identity(self) -> bool {
        (self.x - 1.0).abs() < f64::EPSILON && (self.y - 1.0).abs() < f64::EPSILON
    }
}

/// 若干输出的 CRTC 占位合起来的外接矩形（逻辑空间里的桌面大小）。
pub(crate) fn crtc_extent(geometries: &[OutputGeometry]) -> (u32, u32) {
    let mut width = 0f64;
    let mut height = 0f64;
    for geometry in geometries {
        width = width.max(geometry.bounds.x + geometry.bounds.w);
        height = height.max(geometry.bounds.y + geometry.bounds.h);
    }
    (width.round() as u32, height.round() as u32)
}

impl OutputGeometry {
    /// 稳定标识：连接名优先（重新插拔后不变），没有名字时退回 XID。
    pub(crate) fn key(&self) -> String {
        if self.connector.is_empty() {
            format!("output-{}", self.output_id)
        } else {
            self.connector.clone()
        }
    }

    /// 给人看的名字：RandR 的输出只有连接名。
    pub(crate) fn name(&self) -> String {
        if self.connector.is_empty() {
            self.key()
        } else {
            self.connector.clone()
        }
    }

    /// 旋转后的像素数：模式尺寸按旋转方向摆好，和屏幕上的占位一致。
    pub(crate) fn pixels(&self) -> (u32, u32) {
        if self.rotation_deg == 90 || self.rotation_deg == 270 {
            (self.mode_height, self.mode_width)
        } else {
            (self.mode_width, self.mode_height)
        }
    }

    /// 面板尺寸换算成参考几何。
    ///
    /// 服务器没有 EDID 时会按 96dpi 伪造尺寸，这种尺寸按「读不到」处理。
    pub(crate) fn reference_extent(&self) -> Option<Size> {
        if self.mm_width == 0 || self.mm_height == 0 {
            return None;
        }
        if self.fabricated_size() {
            return None;
        }
        let (pixel_width, pixel_height) = self.pixels();
        extent::reference_extent(
            f64::from(self.mm_width) * 0.001,
            f64::from(self.mm_height) * 0.001,
            &self.bounds,
            pixel_width,
            pixel_height,
        )
    }

    /// 面板尺寸是不是服务器按 96dpi 伪造的（没有 EDID 时的常见做法）。
    ///
    /// 毫米数是按面板自身的方向报的，所以这里用未旋转的模式像素数来比。
    fn fabricated_size(&self) -> bool {
        let dpi = |mm: u32, pixels: u32| f64::from(pixels) * MM_PER_INCH / f64::from(mm);
        (dpi(self.mm_width, self.mode_width) - FABRICATED_DPI).abs() < 0.5
            && (dpi(self.mm_height, self.mode_height) - FABRICATED_DPI).abs() < 0.5
    }

    /// 变成一块显示器。
    pub(crate) fn display_info(&self, is_primary: bool) -> DisplayInfo {
        let (pixel_width, pixel_height) = self.pixels();
        DisplayInfo {
            key: self.key(),
            name: self.name(),
            bounds: self.bounds,
            pixel_width,
            pixel_height,
            reference_extent: self.reference_extent(),
            rotation_deg: self.rotation_deg,
            is_primary,
            // RandR 不报告输出是不是内置在机器上的。
            is_builtin: false,
            // RandR 也不报告虚拟显示器（远程桌面、虚拟屏软件）。
            is_virtual: false,
        }
    }
    /// 换算到 X11 根窗口的像素空间。
    pub(crate) fn scaled(&self, scale: RootScale) -> OutputGeometry {
        if scale.is_identity() {
            return self.clone();
        }
        let mut scaled = self.clone();
        scaled.bounds = Rect::new(
            self.bounds.x * scale.x,
            self.bounds.y * scale.y,
            self.bounds.w * scale.x,
            self.bounds.h * scale.y,
        );
        scaled.mode_width = (f64::from(self.mode_width) * scale.x).round() as u32;
        scaled.mode_height = (f64::from(self.mode_height) * scale.y).round() as u32;
        scaled
    }
}

/// 把 RandR 的旋转位掩码换算成角度。
pub(crate) fn rotation_deg(bits: u16) -> u16 {
    if bits & ROTATE_90 != 0 {
        90
    } else if bits & ROTATE_180 != 0 {
        180
    } else if bits & ROTATE_270 != 0 {
        270
    } else {
        0
    }
}

/// 选主显示器。
///
/// 优先用服务器报告的主输出（RandR 1.3 的 `RRGetOutputPrimary`）；服务器没有这个请求时，
/// 退回「根窗口原点落在哪个输出上」。
pub(crate) fn select_primary(outputs: &[OutputGeometry], server_primary: Option<u32>) -> Option<u32> {
    if let Some(id) = server_primary
        && outputs.iter().any(|output| output.output_id == id)
    {
        return Some(id);
    }
    outputs
        .iter()
        .find(|output| output.bounds.contains(Point::new(0.0, 0.0)))
        .map(|output| output.output_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(id: u32, connector: &str, bounds: Rect) -> OutputGeometry {
        OutputGeometry {
            output_id: id,
            connector: connector.to_string(),
            bounds,
            mode_width: 1920,
            mode_height: 1080,
            rotation_deg: 0,
            mm_width: 600,
            mm_height: 340,
        }
    }

    #[test]
    fn a_scaled_desktop_keeps_the_root_window_size() {
        // XWayland 把缩放后的桌面报成逻辑尺寸，指针却在根窗口的像素空间里。
        let geometry = output(7, "DP-1", Rect::new(0.0, 0.0, 1280.0, 800.0));
        let scale = RootScale::between((1920, 1200), crtc_extent(std::slice::from_ref(&geometry)));
        assert_eq!((scale.x, scale.y), (1.5, 1.5));
        let scaled = geometry.scaled(scale);
        assert_eq!(scaled.bounds, Rect::new(0.0, 0.0, 1920.0, 1200.0));
        assert_eq!((scaled.mode_width, scaled.mode_height), (2880, 1620));
    }

    #[test]
    fn an_unscaled_desktop_keeps_its_geometry() {
        let geometry = output(7, "DP-1", Rect::new(0.0, 0.0, 1920.0, 1080.0));
        let scale = RootScale::between((1920, 1080), crtc_extent(std::slice::from_ref(&geometry)));
        assert!(scale.is_identity());
        assert_eq!(geometry.scaled(scale), geometry);
    }

    #[test]
    fn an_output_that_does_not_fill_the_root_is_not_a_scaled_desktop() {
        // 两个方向的比例不一致：那是输出没铺满根窗口，不是桌面缩放。
        let geometry = output(7, "DP-1", Rect::new(0.0, 0.0, 1920.0, 1080.0));
        let scale = RootScale::between((1920, 1200), crtc_extent(std::slice::from_ref(&geometry)));
        assert!(scale.is_identity());
    }

    #[test]
    fn the_crtc_extent_covers_every_output() {
        let first = output(1, "DP-1", Rect::new(0.0, 0.0, 1280.0, 800.0));
        let second = output(2, "HDMI-1", Rect::new(1280.0, 0.0, 640.0, 800.0));
        assert_eq!(crtc_extent(&[first, second]), (1920, 800));
    }
    #[test]
    fn rotation_bits_map_to_degrees() {
        assert_eq!(rotation_deg(0), 0);
        assert_eq!(rotation_deg(1 << 0), 0);
        assert_eq!(rotation_deg(1 << 1), 90);
        assert_eq!(rotation_deg(1 << 2), 180);
        assert_eq!(rotation_deg(1 << 3), 270);
    }

    #[test]
    fn rotated_output_swaps_pixels_and_keeps_the_footprint() {
        let mut geometry = output(7, "DP-1", Rect::new(0.0, 0.0, 1080.0, 1920.0));
        geometry.rotation_deg = 90;
        geometry.mode_width = 1920;
        geometry.mode_height = 1080;
        let info = geometry.display_info(true);
        assert_eq!((info.pixel_width, info.pixel_height), (1080, 1920));
        assert_eq!(info.rotation_deg, 90);
        assert_eq!(info.bounds, Rect::new(0.0, 0.0, 1080.0, 1920.0));
    }

    #[test]
    fn upright_output_keeps_the_mode_pixels() {
        let geometry = output(7, "DP-1", Rect::new(0.0, 0.0, 1920.0, 1080.0));
        let info = geometry.display_info(false);
        assert_eq!((info.pixel_width, info.pixel_height), (1920, 1080));
        assert_eq!(info.rotation_deg, 0);
    }

    #[test]
    fn key_and_name_come_from_the_connector() {
        let geometry = output(7, "DP-1", Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(geometry.key(), "DP-1");
        assert_eq!(geometry.name(), "DP-1");
    }

    #[test]
    fn key_falls_back_to_the_output_id() {
        let geometry = output(7, "", Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(geometry.key(), "output-7");
        assert_eq!(geometry.name(), "output-7");
    }

    #[test]
    fn primary_prefers_the_server_report() {
        let first = output(1, "DP-1", Rect::new(0.0, 0.0, 1920.0, 1080.0));
        let second = output(2, "HDMI-1", Rect::new(1920.0, 0.0, 1920.0, 1080.0));
        let outputs = [first, second];
        assert_eq!(select_primary(&outputs, Some(2)), Some(2));
        // 服务器报告的主输出没接上时，退回根窗口原点。
        assert_eq!(select_primary(&outputs, Some(9)), Some(1));
    }

    #[test]
    fn primary_falls_back_to_the_root_origin() {
        let first = output(1, "DP-1", Rect::new(-1920.0, 0.0, 1920.0, 1080.0));
        let second = output(2, "HDMI-1", Rect::new(0.0, 0.0, 1920.0, 1080.0));
        let outputs = [first, second];
        assert_eq!(select_primary(&outputs, None), Some(2));
    }

    #[test]
    fn primary_is_absent_when_nothing_covers_the_root_origin() {
        let only = output(1, "DP-1", Rect::new(1920.0, 0.0, 1920.0, 1080.0));
        assert_eq!(select_primary(&[only], None), None);
    }

    #[test]
    fn reported_millimetres_become_the_reference_extent() {
        let geometry = output(7, "DP-1", Rect::new(0.0, 0.0, 1920.0, 1080.0));
        let extent = geometry.reference_extent().unwrap();
        assert_eq!(extent.w, 0.600 / oink_foundation::display::REFERENCE_DIAGONAL_METERS);
        assert_eq!(extent.h, 0.340 / oink_foundation::display::REFERENCE_DIAGONAL_METERS);
    }

    #[test]
    fn fabricated_size_is_ignored() {
        let mut geometry = output(7, "DP-1", Rect::new(0.0, 0.0, 1920.0, 1080.0));
        // 服务器按 96dpi 伪造：1920 像素 = 508 毫米。
        geometry.mm_width = 508;
        geometry.mm_height = 286;
        assert!(geometry.reference_extent().is_none());
    }

    #[test]
    fn missing_millimetres_are_ignored() {
        let mut geometry = output(7, "DP-1", Rect::new(0.0, 0.0, 1920.0, 1080.0));
        geometry.mm_width = 0;
        geometry.mm_height = 0;
        assert!(geometry.reference_extent().is_none());
    }

    #[test]
    fn rotated_extent_follows_the_footprint() {
        let mut geometry = output(7, "DP-1", Rect::new(0.0, 0.0, 1080.0, 1920.0));
        geometry.rotation_deg = 90;
        geometry.mode_width = 1920;
        geometry.mode_height = 1080;
        let extent = geometry.reference_extent().unwrap();
        assert_eq!(extent.w, 0.340 / oink_foundation::display::REFERENCE_DIAGONAL_METERS);
        assert_eq!(extent.h, 0.600 / oink_foundation::display::REFERENCE_DIAGONAL_METERS);
    }
}
