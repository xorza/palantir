//! Reading a `PaintCapture` back: what counts as a rect, a shadow, a clip
//! pair.

use crate::primitives::color::ColorF16;
use crate::primitives::{rect::Rect, translate_scale::TranslateScale};
use crate::renderer::frontend::capture::{PaintCall, PaintCapture};
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::draw_quad_payload::QuadGeom;

/// A plain rectangle draw. Rects, shadows, and triangles all record as
/// [`PaintCall::Quad`] now, and a shadow shares the rect *geometry* — so
/// isolating a rectangle takes both tests: rect geometry, and a fill
/// kind that isn't the shadow SDF.
pub(super) fn as_rect(call: &PaintCall) -> Option<&DrawQuadPayload> {
    match call {
        PaintCall::Quad(p)
            if matches!(p.geom, QuadGeom::Rect { .. }) && !p.fill.kind.is_shadow() =>
        {
            Some(p)
        }
        _ => None,
    }
}

pub(super) fn count_draw_rects(cmds: &PaintCapture) -> usize {
    cmds.calls.iter().filter(|c| as_rect(c).is_some()).count()
}

/// Walk a recorded paint stream and return the effective screen-space rect
/// for each `Rect` call, keyed by its fill colour.
pub(super) fn screen_rects_by_fill(cmds: &PaintCapture) -> Vec<(ColorF16, Rect)> {
    let mut t = TranslateScale::IDENTITY;
    let mut t_stack: Vec<TranslateScale> = Vec::new();
    let mut clip: Option<Rect> = None;
    let mut clip_stack: Vec<Option<Rect>> = Vec::new();
    let mut out = Vec::new();
    for command in cmds.calls.iter() {
        match command {
            PaintCall::PushTransform(child) => {
                t_stack.push(t);
                t = t.compose(*child);
            }
            PaintCall::PopTransform => t = t_stack.pop().expect("balanced PushTransform/Pop"),
            PaintCall::PushClip(p) => {
                let screen = t.apply_rect(p.rect);
                let intersected = match clip {
                    Some(c) => screen.clamp_to(c),
                    None => screen,
                };
                clip_stack.push(clip);
                clip = Some(intersected);
            }
            PaintCall::PopClip => clip = clip_stack.pop().expect("balanced PushClip/Pop"),
            // Rectangles only — shadows and triangles ride the same
            // call now, and `as_rect` is what still separates them.
            call if as_rect(call).is_some() => {
                let p = as_rect(call).unwrap();
                let screen = t.apply_rect(quad_rect(p));
                let visible = match clip {
                    Some(c) => screen.clamp_to(c),
                    None => screen,
                };
                out.push((p.fill.color, visible));
            }
            PaintCall::Quad(_)
            | PaintCall::Text(_)
            | PaintCall::Mesh(_)
            | PaintCall::Polyline(_)
            | PaintCall::Image { .. }
            | PaintCall::Icon(_)
            | PaintCall::Curve(_) => {}
        }
    }
    assert!(t_stack.is_empty(), "transform stack unbalanced");
    assert!(clip_stack.is_empty(), "clip stack unbalanced");
    out
}

/// The logical-px paint rect of a rect-geometry quad.
#[track_caller]
pub(super) fn quad_rect(p: &DrawQuadPayload) -> Rect {
    match p.geom {
        QuadGeom::Rect { rect, .. } => rect,
        QuadGeom::Triangle { .. } => panic!("expected rect geometry, got a triangle"),
    }
}
