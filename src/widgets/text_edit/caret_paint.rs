//! The caret as the painter draws it.

use crate::primitives::color::Color;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::text::probe::Caret;

#[derive(Clone, Copy, Debug)]
pub(super) struct CaretPaint {
    pub(super) pos: Caret,
    pub(super) width: f32,
    pub(super) color: Color,
    pub(super) anim: Option<PaintAnim>,
}
