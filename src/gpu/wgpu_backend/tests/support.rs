//! Building a `RenderBuffer` by hand, and reading the emitted steps back.

use crate::common::span::Span;
use crate::display::Display;
use crate::gpu::frame::schedule::{MaskPlan, RenderStep, for_each_step};
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
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

/// Simplified view of the render schedule: bookkeeping steps (`SetScissor`, `SetStencilRef`) are stripped.
/// Raw [`RenderStep`] is tested where scissor narrowing and stencil-ref stepping matter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DrawOp {
    PreClear,
    MaskWrite(u32),
    MaskClear(u32),
    Quads(usize),
    Shadows(usize),
    Text(usize),
    Meshes(usize),
    Images(usize),
    Icons(usize),
    Curves(usize),
}

pub(super) fn collect(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    masks: Option<&MaskPlan>,
) -> Vec<RenderStep> {
    let mut steps = Vec::new();
    for_each_step(buffer, damage_scissor, masks, &mut |s| {
        steps.push(s);
    });
    steps
}

pub(super) fn plain_steps(buffer: &RenderBuffer) -> Vec<RenderStep> {
    collect(buffer, None, None)
}

pub(super) fn simplify(buffer: &RenderBuffer, steps: &[RenderStep]) -> Vec<DrawOp> {
    // A quad step names its group by span, so groups sharing a non-empty span would be ambiguous.
    for (i, group) in buffer.groups.iter().enumerate() {
        debug_assert!(
            group.quads.len == 0
                || !buffer.groups[..i]
                    .iter()
                    .any(|earlier| earlier.quads == group.quads),
            "groups {i} and an earlier one share the quad span {:?}",
            group.quads,
        );
    }
    let mut out = Vec::new();
    for s in steps {
        match s {
            RenderStep::PreClear => out.push(DrawOp::PreClear),
            RenderStep::SetScissor(_) | RenderStep::SetStencilRef(_) => {}
            RenderStep::MaskStamp(mi) => out.push(DrawOp::MaskWrite(*mi)),
            RenderStep::MaskClear(mi) => out.push(DrawOp::MaskClear(*mi)),
            RenderStep::Quads { range } => out.push(DrawOp::Quads(group_of(buffer, *range))),
            RenderStep::Shadows { range } => {
                out.push(DrawOp::Shadows(group_of(buffer, *range)));
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

/// The group whose quad span holds `run` (a run is the whole span unless it mixes shadows with other quads).
fn group_of(buffer: &RenderBuffer, run: Span) -> usize {
    buffer
        .groups
        .iter()
        .position(|group| {
            group.quads.len != 0
                && group.quads.start <= run.start
                && run.start + run.len <= group.quads.start + group.quads.len
        })
        .expect("quad run outside every draw group")
}

pub(super) fn scissor_count(steps: &[RenderStep]) -> usize {
    steps
        .iter()
        .filter(|s| matches!(s, RenderStep::SetScissor(_)))
        .count()
}

fn dummy_quad() -> Quad {
    Quad {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        fill: RgbaF32::WHITE.into(),
        corners: Corners::ZERO,
        stroke_color: RgbaF16::TRANSPARENT,
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
        color: RgbaF16::from(RgbaF32::WHITE),
        scale: 1.0,
    }
}

pub(super) fn group(quads: Span) -> DrawGroup {
    DrawGroup {
        scissor: Some(URect::new(0, 0, 100, 100)),
        rounded_clips: Span::default(),
        quads,
    }
}

/// A 100×100 buffer with the given groups and no text batches; pools have four slots so any small span is valid.
pub(super) fn buf_with(groups: Vec<DrawGroup>) -> RenderBuffer {
    buf_with_batches(groups, Vec::new())
}

/// A `TextBatch` with the full-viewport sentinel scissor and no mask chain. Built explicitly, as the composer
/// emits them (`DrawGroup` has no text span; deriving them would mask composer/batch decorrelation bugs).
pub(super) fn text_batch(texts: Span, last_group: u32) -> TextBatch {
    TextBatch {
        texts,
        last_group,
        scissor: URect::new(0, 0, u32::MAX, u32::MAX),
        rounded_clips: Span::default(),
    }
}

pub(super) fn buf_with_tier_anchors(
    groups: Vec<DrawGroup>,
    tier: PaintTier,
    anchors: &[u32],
) -> RenderBuffer {
    let mut buf = buf_with(groups);
    for (i, &g) in anchors.iter().enumerate() {
        buf.batches_mut(tier).push(GroupBatch {
            items: Span::new(i as u32, 1),
            last_group: g,
        });
    }
    buf
}

/// A 100×100 buffer with explicit `text_batches` (see [`text_batch`]).
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
