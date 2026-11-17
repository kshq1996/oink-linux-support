//! Linux has no native projection window.
//!
//! A layout projection is a separate always-on-top window that draws the
//! arrangement of the screens. Linux reports no backend, so the product
//! shows the arrangement inside its own window instead of drawing a surface
//! that would sit above every other application.
use std::sync::Arc;

use oink_foundation::Rect;

use super::Frame;
use crate::diagnostics::Health;

pub(super) struct Backend;

impl Backend {
    pub(super) fn new(_health: Arc<Health>) -> Option<Self> {
        None
    }

    pub(super) fn present(&mut self, _frames: Vec<Frame>) {}

    pub(super) fn hide(&mut self) {}

    pub(super) fn pump(&mut self) {}
}

/// X11 desktop coordinates are pixels. The design's points are the pixels of
/// a 96 DPI display, so the unit is one pixel per point without a scale.
pub(super) fn annotation_unit(_screen: Rect) -> f64 {
    1.0
}
