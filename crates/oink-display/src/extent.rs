//! Panel measurements shared by the platform backends.
//!
//! macOS reports the panel size through `CGDisplayScreenSize`, Linux through
//! the RandR output's millimetre fields. Both need the same conversion into
//! the reference geometry and the same rejection of the fabricated 72-dpi
//! values that a machine without an EDID reports.

use oink_foundation::geom::{Rect, Size};

/// 宽高方向与 `bounds` 不一致时需要交换（旋转 90/270° 的屏幕）。
pub(crate) fn needs_swap(w: f64, h: f64, bounds: &Rect) -> bool {
    w > 0.0 && h > 0.0 && bounds.w != bounds.h && w != h && ((w > h) != (bounds.w > bounds.h))
}

/// Convert measured metres into reference geometry, oriented like native bounds.
/// Reject missing measurements and the system's fabricated 72-dpi EDID fallback.
pub(crate) fn reference_extent(w: f64, h: f64, bounds: &Rect, pixel_w: u32, pixel_h: u32) -> Option<Size> {
    if !(w.is_finite() && h.is_finite()) || w <= 0.001 || h <= 0.001 {
        return None;
    }
    let (w, h) = if needs_swap(w, h, bounds) { (h, w) } else { (w, h) };
    const FAKE_PIXELS_PER_METER: f64 = 2835.0;
    let is_fake = |len: f64, units: f64| units > 0.0 && (units / FAKE_PIXELS_PER_METER - len).abs() < 0.001;
    if (is_fake(w, bounds.w) && is_fake(h, bounds.h))
        || (is_fake(w, f64::from(pixel_w)) && is_fake(h, f64::from(pixel_h)))
    {
        return None;
    }
    Some(Size::new(
        w / oink_foundation::display::REFERENCE_DIAGONAL_METERS,
        h / oink_foundation::display::REFERENCE_DIAGONAL_METERS,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_extent_follows_rotation() {
        let portrait = Rect::new(0.0, 0.0, 1080.0, 1920.0);
        let s = reference_extent(0.600, 0.340, &portrait, 1080, 1920).unwrap();
        assert_eq!((s.w, s.h), (0.340 / 0.686, 0.600 / 0.686));
        let landscape = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        let s = reference_extent(0.600, 0.340, &landscape, 1920, 1080).unwrap();
        assert_eq!((s.w, s.h), (0.600 / 0.686, 0.340 / 0.686));
    }

    #[test]
    fn fake_edid_size_is_rejected() {
        let b = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        assert!(reference_extent(1920.0 / 2835.0, 1080.0 / 2835.0, &b, 1920, 1080).is_none());
        assert!(reference_extent(0.0, 0.0, &b, 1920, 1080).is_none());
    }
}
