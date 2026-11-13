//! Shaped text records consumed by the native text backend.

use crate::primitives::geometry::urect::URect;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::clip_chain::ClipChain;
use crate::renderer::render_buffer::row_spin::RowSpin;
use crate::text::shaped_ref::ShapedTextRef;
use glam::Vec2;

/// One shaped text run in physical-px space. The backend resolves
/// [`ShapedTextRef`] when the encoded glyph cache misses.
///
/// Not `TextRun`: that is the authoring type [`crate::widget::TextRun`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextDrawRow {
    pub(crate) text: ShapedTextRef,
    pub(crate) origin: Vec2,
    /// Clip bounds (physical px). An unturned run culls its lines against them and, where they cut
    /// its [`Self::reach`], clips its glyphs' quads to them on the CPU (`RasterPass::clip_since`);
    /// a turned run culls none, and its batch's run's scissor clips it
    /// ([`DrawRun::scissor`](crate::renderer::render_buffer::draw_run::DrawRun::scissor)), which
    /// equals these bounds once a clip cuts the run.
    pub(crate) bounds: URect,
    pub(crate) color: RgbaF16,
    /// Per-run scale on top of the DPI scale: the ancestor `TranslateScale.scale`
    /// snapped to `TEXT_SCALE_STEP` rungs (`composer::geometry::snap_text_scale`).
    /// `1.0` outside transformed subtrees. Each distinct value mints a new glyph
    /// rasterization, so snapping keeps zoom gestures from re-rasterizing every frame.
    pub(crate) scale: f32,
    /// A paint animation's turn of the whole run, about a physical pivot.
    pub(crate) spin: Option<RowSpin>,
    /// The rounded clips that cut the run, which each of its glyphs carries.
    pub(crate) clip: ClipChain,
    /// Whether its bounds, a clip's or the viewport's edge, cut the run's reach, its glyphs'
    /// padded ink: the backend clips a cut run's glyph quads to them, and passes over a whole one
    /// under no rounded clip, whose quads lie inside them already.
    pub(crate) reach: TextClip,
}

/// Whether an edge cuts a text run, which its glyphs or its batch's scissor then clip it to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextClip {
    Whole,
    Cut,
}
