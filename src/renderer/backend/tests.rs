//! Backend draw-schedule tests. The backend's `submit` method does GPU
//! work that's hard to inspect in a unit test; instead we mirror its
//! draw-ordering logic in the pure `render_schedule` helper here and
//! pin the order against expected sequences.

use crate::layout::types::span::Span;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::urect::URect;
use crate::renderer::gpu::buffer::{DrawGroup, RenderBuffer, TextRun};
use crate::renderer::gpu::quad::Quad;
use crate::text::TextCacheKey;
use glam::{UVec2, Vec2};

/// One step of the backend's per-frame draw schedule. Used here to pin
/// draw ordering without a GPU. `Quads(i)` draws group `i`'s quads;
/// `Text(i)` renders text scoped to group `i`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderStep {
    Quads(usize),
    Text(usize),
}

/// Pure function describing the order of operations
/// [`super::WgpuBackend::submit`] performs on `buffer`. Per-group: emit
/// `Quads(i)` if the group has quads, then `Text(i)` if the group has
/// text. Mirrors the loop in `submit`.
fn render_schedule(buffer: &RenderBuffer) -> Vec<RenderStep> {
    let mut steps = Vec::new();
    for (i, g) in buffer.groups.iter().enumerate() {
        if g.quads.len != 0 {
            steps.push(RenderStep::Quads(i));
        }
        if g.texts.len != 0 {
            steps.push(RenderStep::Text(i));
        }
    }
    steps
}

fn dummy_quad() -> Quad {
    Quad::new(
        Rect::new(0.0, 0.0, 10.0, 10.0),
        Color::WHITE,
        Corners::ZERO,
        None,
    )
}

fn dummy_text() -> TextRun {
    TextRun {
        origin: glam::Vec2::ZERO,
        bounds: URect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        },
        color: Color::WHITE,
        key: TextCacheKey::INVALID,
    }
}

/// Pin: text in group 0 renders *between* group 0's quads and group 1's
/// quads, so a child quad declared after a label can occlude it. This
/// is the per-group z-order contract — the showcase tab `text z-order`
/// demonstrates the visual outcome.
#[test]
fn render_schedule_interleaves_text_per_group() {
    let buf = RenderBuffer {
        quads: vec![dummy_quad(); 3],
        texts: vec![dummy_text()],
        groups: vec![
            // Group 0: 2 quads + 1 text
            DrawGroup {
                scissor: None,
                rounded_clip: None,
                quads: Span::new(0, 2),
                texts: Span::new(0, 1),
            },
            // Group 1: 1 quad, no text
            DrawGroup {
                scissor: None,
                rounded_clip: None,
                quads: Span::new(2, 1),
                texts: Span::new(1, 0),
            },
        ],
        viewport_phys: UVec2::new(100, 100),
        viewport_phys_f: Vec2::new(100.0, 100.0),
        scale: 1.0,
        has_rounded_clip: false,
    };
    assert_eq!(
        render_schedule(&buf),
        vec![
            RenderStep::Quads(0),
            RenderStep::Text(0),
            RenderStep::Quads(1),
        ],
    );
}

/// Edge case: a group with text but no quads (e.g. a Hug parent whose
/// only paint is its label). Schedule must still emit `Text(i)`.
#[test]
fn render_schedule_emits_text_for_quadless_group() {
    let buf = RenderBuffer {
        quads: vec![dummy_quad()],
        texts: vec![dummy_text(); 2],
        groups: vec![
            // Group 0: 1 quad only
            DrawGroup {
                scissor: None,
                rounded_clip: None,
                quads: Span::new(0, 1),
                texts: Span::new(0, 0),
            },
            // Group 1: text only, no quads
            DrawGroup {
                scissor: None,
                rounded_clip: None,
                quads: Span::new(1, 0),
                texts: Span::new(0, 2),
            },
        ],
        viewport_phys: UVec2::new(100, 100),
        viewport_phys_f: Vec2::new(100.0, 100.0),
        scale: 1.0,
        has_rounded_clip: false,
    };
    assert_eq!(
        render_schedule(&buf),
        vec![RenderStep::Quads(0), RenderStep::Text(1)],
    );
}
