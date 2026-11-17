//! Linux has no native toast surface.
//!
//! The toast queue and its rasterizer are platform neutral, but a desktop
//! toast needs a window the compositor draws above the other windows. Linux
//! reports no surface, so the product shows the toast inside its own window
//! instead of pretending a desktop toast was shown.
use super::{Click, Frame, Surface};

pub(super) struct Backend;

impl Backend {
    pub fn new() -> Option<Self> {
        None
    }
}

impl Surface for Backend {
    fn present(&mut self, _frames: Vec<Frame>) {}
    fn clicks(&mut self) -> Vec<Click> {
        Vec::new()
    }
    fn reduced_motion(&self) -> bool {
        false
    }
}
