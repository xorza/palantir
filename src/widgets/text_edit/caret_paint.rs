//! The caret as the painter draws it.

use crate::primitives::color::RgbaF32;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::text::probe::Caret;

#[derive(Clone, Copy, Debug)]
pub(super) struct CaretPaint {
    pub(super) pos: Caret,
    pub(super) width: f32,
    pub(super) color: RgbaF32,
    pub(super) anim: Option<PaintAnim>,
}
