//! Backend draw-schedule tests. The backend's `submit` method does GPU
//! work that's hard to inspect in a unit test; instead we mirror its
//! draw-ordering logic in the pure `render_schedule` helper here and
//! pin the order against expected sequences.

use crate::primitives::Color;
use crate::renderer::buffer::{DrawGroup, RenderBuffer, ScissorRect, TextRun};
use crate::renderer::quad::Quad;
use crate::text::TextCacheKey;

/// One step of the backend's per-frame draw schedule. Used here to pin
/// draw ordering without a GPU. `Quads(i)` draws group `i`'s quads;
/// `Text(i)` renders text scoped to group `i`. The sentinel
/// `Text(usize::MAX)` means "all text at the end of frame, ungrouped" —
/// the v1 limitation that ignores per-group text ordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderStep {
    Quads(usize),
    Text(usize),
}

/// Pure function describing the order of operations
/// [`super::WgpuBackend::submit`] performs on `buffer`. Captures today's
/// behavior: quads drawn group by group in declaration order, then a
/// single global text render at the end (see `backend/text.rs` v1
/// limitation). Per-group text rendering would replace the trailing
/// `Text(usize::MAX)` with `Text(i)` entries interleaved after each
/// `Quads(i)`.
fn render_schedule(buffer: &RenderBuffer) -> Vec<RenderStep> {
    let mut steps = Vec::new();
    for (i, g) in buffer.groups.iter().enumerate() {
        if !g.quads.is_empty() {
            steps.push(RenderStep::Quads(i));
        }
    }
    if !buffer.texts.is_empty() {
        steps.push(RenderStep::Text(usize::MAX));
    }
    steps
}

fn dummy_quad() -> Quad {
    Quad::new(
        crate::primitives::Rect::new(0.0, 0.0, 10.0, 10.0),
        Color::WHITE,
        crate::primitives::Corners::ZERO,
        None,
    )
}

fn dummy_text() -> TextRun {
    TextRun {
        origin: [0.0, 0.0],
        bounds: ScissorRect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        },
        color: Color::WHITE,
        key: TextCacheKey::INVALID,
    }
}

/// Pin: today's schedule. All quads in group order, then one global
/// `Text` step at the end ignoring per-group ordering.
#[test]
fn render_schedule_today_renders_text_globally_at_end() {
    let buf = RenderBuffer {
        quads: vec![dummy_quad(); 3],
        texts: vec![dummy_text()],
        groups: vec![
            DrawGroup {
                scissor: None,
                quads: 0..2,
                texts: 0..1,
            },
            DrawGroup {
                scissor: None,
                quads: 2..3,
                texts: 1..1,
            },
        ],
        viewport_phys: [100, 100],
        viewport_phys_f: [100.0, 100.0],
        scale: 1.0,
    };
    assert_eq!(
        render_schedule(&buf),
        vec![
            RenderStep::Quads(0),
            RenderStep::Quads(1),
            RenderStep::Text(usize::MAX),
        ],
    );
}

/// Spec for the per-group text z-order fix (see `docs/text.md`,
/// "Per-group text z-order" open question, and the `text z-order`
/// showcase tab). Text in group 0 must render *between* group 0's
/// quads and group 1's quads, so a child quad declared after a label
/// can occlude it.
///
/// Currently fails: today's `render_schedule` emits all quads first
/// then a single trailing `Text(usize::MAX)`. Once the backend is
/// rewritten to interleave per-group `prepare`/`render` (Option D —
/// pool of `glyphon::TextRenderer`s sharing one atlas), this test
/// passes and `#[ignore]` should be removed.
///
/// Run with `cargo test --include-ignored render_schedule_interleaves`.
#[test]
#[ignore = "spec for per-group text z-order — see text z-order showcase"]
fn render_schedule_interleaves_text_per_group() {
    let buf = RenderBuffer {
        quads: vec![dummy_quad(); 3],
        texts: vec![dummy_text()],
        groups: vec![
            // Group 0: 2 quads + 1 text
            DrawGroup {
                scissor: None,
                quads: 0..2,
                texts: 0..1,
            },
            // Group 1: 1 quad, no text
            DrawGroup {
                scissor: None,
                quads: 2..3,
                texts: 1..1,
            },
        ],
        viewport_phys: [100, 100],
        viewport_phys_f: [100.0, 100.0],
        scale: 1.0,
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
