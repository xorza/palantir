//! Building a `RenderBuffer` by hand, and reading the emitted steps back.

use crate::display::Display;
use crate::primitives::color::{Color, ColorF16, ColorU8};
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::urect::URect;
use crate::renderer::backend::schedule::{MaskPlan, RenderStep, for_each_step};
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::draw_group::DrawGroup;
use crate::renderer::render_buffer::group_batch::GroupBatch;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::renderer::render_buffer::text_batch::TextBatch;
use crate::text::key::TextShapeKey;
use crate::text::shaped_ref::ShapedTextRef;
use glam::UVec2;

/// "Simplified" view of the render schedule — strips bookkeeping
/// (`SetScissor`, `SetStencilRef`) that the tests don't care to pin
/// directly; `MaskStamp` / `MaskClear` map to `MaskWrite` /
/// `MaskClear`. Stencil tests assert on this view; raw [`RenderStep`]
/// is also tested (e.g. `scissor_steps_emit_once_per_transition`) for
/// fidelity that scissor narrowing and stencil-ref stepping actually
/// happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DrawOp {
    PreClear,
    MaskWrite(u32),
    MaskClear(u32),
    Quads(usize),
    Text(usize),
    Meshes(usize),
    Images(usize),
    Icons(usize),
    Curves(usize),
}

pub(super) fn collect(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    masks: &MaskPlan,
    use_stencil: bool,
) -> Vec<RenderStep> {
    let mut steps = Vec::new();
    for_each_step(buffer, damage_scissor, masks, use_stencil, &mut |s| {
        steps.push(s);
    });
    steps
}

pub(super) fn simplify(buffer: &RenderBuffer, steps: &[RenderStep]) -> Vec<DrawOp> {
    let mut out = Vec::new();
    for s in steps {
        match s {
            RenderStep::PreClear => out.push(DrawOp::PreClear),
            RenderStep::SetScissor(_) | RenderStep::SetStencilRef(_) => {}
            RenderStep::MaskStamp(mi) => out.push(DrawOp::MaskWrite(*mi)),
            RenderStep::MaskClear(mi) => out.push(DrawOp::MaskClear(*mi)),
            RenderStep::Quads { range } => {
                let group = buffer
                    .groups
                    .iter()
                    .position(|candidate| candidate.quads == *range)
                    .expect("quad range missing from draw groups");
                out.push(DrawOp::Quads(group));
            }
            RenderStep::Text { batch } => out.push(DrawOp::Text(*batch)),
            RenderStep::TierBatch { tier, batch } => {
                let group = buffer.batches(*tier)[*batch].last_group as usize;
                out.push(match tier {
                    PaintTier::Mesh => DrawOp::Meshes(group),
                    PaintTier::Image => DrawOp::Images(group),
                    PaintTier::Icon => DrawOp::Icons(group),
                    PaintTier::Curve => DrawOp::Curves(group),
                });
            }
        }
    }
    out
}

/// Number of `SetScissor` steps in `steps` — the metric the scissor
/// deduplication is about.
pub(super) fn scissor_count(steps: &[RenderStep]) -> usize {
    steps
        .iter()
        .filter(|s| matches!(s, RenderStep::SetScissor(_)))
        .count()
}

fn dummy_quad() -> Quad {
    Quad {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        fill: Color::WHITE.into(),
        corners: Corners::ZERO,
        stroke_color: ColorF16::TRANSPARENT,
        stroke_width: 0.0,
        ..Default::default()
    }
}

fn dummy_text() -> TextDrawRow {
    TextDrawRow {
        origin: glam::Vec2::ZERO,
        bounds: URect::ZERO,
        text: ShapedTextRef {
            key: TextShapeKey::fixture(),
            span: Span::default(),
        },
        color: ColorU8::WHITE,
        scale: 1.0,
    }
}

/// Builds a 100×100 buffer with the given groups and no text batches.
/// Quads/texts pools have four slots each so any small span is valid.
pub(super) fn buf_with(groups: Vec<DrawGroup>) -> RenderBuffer {
    buf_with_batches(groups, Vec::new())
}

/// A `TextBatch` with the full-viewport sentinel scissor and no mask
/// chain — schedule tests don't drive shader-level clipping, so the
/// scissor only needs to survive the damage intersect. Text batches
/// are constructed explicitly (mirroring what the composer emits)
/// rather than derived from groups: `DrawGroup` carries no per-group
/// text span, and a fixture that synthesized batches from groups
/// would mask composer/batch decorrelation bugs. Batches anchored at
/// *rounded* groups build their `TextBatch` inline instead: they need
/// a chain matching their `last_group`'s and a realistic bounds-union
/// scissor (the composer clamps it to the clip, so it never exceeds
/// the stamp scissor the way this sentinel would).
pub(super) fn text_batch(texts: Span, last_group: u32) -> TextBatch {
    TextBatch {
        texts,
        last_group,
        scissor: URect::new(0, 0, u32::MAX, u32::MAX),
        rounded_clips: Span::default(),
    }
}

/// Same shape as [`buf_with_mesh_anchors`] but for image batches.
pub(super) fn buf_with_image_anchors(groups: Vec<DrawGroup>, anchors: &[u32]) -> RenderBuffer {
    let mut buf = buf_with(groups);
    for (i, &g) in anchors.iter().enumerate() {
        buf.batches_mut(PaintTier::Image).push(GroupBatch {
            items: Span::new(i as u32, 1),
            last_group: g,
        });
    }
    buf
}

/// Constructs a 100×100 buffer with the given groups and explicit
/// `text_batches` (built the way the composer would emit them — see
/// [`text_batch`]). Quads/texts pools have four slots each so any
/// small span is valid.
pub(super) fn buf_with_batches(
    groups: Vec<DrawGroup>,
    text_batches: Vec<TextBatch>,
) -> RenderBuffer {
    let mut buffer = RenderBuffer::new();
    buffer.quads = vec![dummy_quad(); 4];
    buffer.texts = vec![dummy_text(); 4];
    buffer.groups = groups;
    buffer.text_batches = text_batches;
    buffer.display = Display::from_physical(UVec2::new(100, 100), 1.0);
    buffer
}
