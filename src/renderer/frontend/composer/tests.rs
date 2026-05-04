use super::super::cmd_buffer::{
    DrawRectPayload, DrawRectStrokedPayload, DrawTextPayload, RenderCmdBuffer,
};
use super::Composer;
use crate::primitives::display::Display;
use crate::primitives::{color::Color, corners::Corners, rect::Rect, urect::URect};
use crate::renderer::buffer::RenderBuffer;
use crate::test_support::RenderCmd;
use crate::text::TextCacheKey;
use glam::UVec2;

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y, w, h)
}

fn draw(r: Rect) -> RenderCmd {
    RenderCmd::DrawRect(DrawRectPayload {
        rect: r,
        radius: Corners::ZERO,
        fill: Color::rgb(1.0, 1.0, 1.0),
    })
}

fn text(r: Rect) -> RenderCmd {
    RenderCmd::DrawText(DrawTextPayload {
        rect: r,
        color: Color::WHITE,
        key: TextCacheKey::INVALID,
    })
}

fn params(scale: f32, viewport_phys: [u32; 2]) -> Display {
    Display {
        physical: UVec2::new(viewport_phys[0], viewport_phys[1]),
        scale_factor: scale,
        pixel_snap: false,
    }
}

fn run(cmds: &[RenderCmd], display: &Display) -> RenderBuffer {
    let mut buffer = RenderCmdBuffer::default();
    for c in cmds {
        match *c {
            RenderCmd::PushClip(r) => buffer.push_clip(r),
            RenderCmd::PopClip => buffer.pop_clip(),
            RenderCmd::PushTransform(t) => buffer.push_transform(t),
            RenderCmd::PopTransform => buffer.pop_transform(),
            RenderCmd::DrawRect(DrawRectPayload { rect, radius, fill }) => {
                buffer.draw_rect(rect, radius, fill, None)
            }
            RenderCmd::DrawRectStroked(DrawRectStrokedPayload {
                rect,
                radius,
                fill,
                stroke,
            }) => buffer.draw_rect(rect, radius, fill, Some(stroke)),
            RenderCmd::DrawText(DrawTextPayload { rect, color, key }) => {
                buffer.draw_text(rect, color, key)
            }
        }
    }
    let mut composer = Composer::default();
    composer.compose(&buffer, display);
    std::mem::take(&mut composer.buffer)
}

#[test]
fn compose_with_no_clip_emits_one_unscissored_group() {
    let buf = run(
        &[
            draw(rect(0.0, 0.0, 10.0, 10.0)),
            draw(rect(20.0, 0.0, 10.0, 10.0)),
        ],
        &params(1.0, [200, 200]),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.groups.len(), 1);
    assert!(buf.groups[0].scissor.is_none());
    assert_eq!(buf.groups[0].quads, 0..2);
}

#[test]
fn compose_with_clip_groups_inner_draws_under_scissor() {
    let buf = run(
        &[
            draw(rect(0.0, 0.0, 10.0, 10.0)),
            RenderCmd::PushClip(rect(50.0, 50.0, 100.0, 100.0)),
            draw(rect(60.0, 60.0, 20.0, 20.0)),
            draw(rect(90.0, 90.0, 20.0, 20.0)),
            RenderCmd::PopClip,
            draw(rect(0.0, 0.0, 5.0, 5.0)),
        ],
        &params(1.0, [400, 400]),
    );
    assert_eq!(buf.quads.len(), 4);
    assert_eq!(buf.groups.len(), 3);

    assert!(buf.groups[0].scissor.is_none());
    assert_eq!(buf.groups[0].quads, 0..1);

    let s = buf.groups[1]
        .scissor
        .expect("clipped group must have a scissor");
    assert_eq!((s.x, s.y, s.w, s.h), (50, 50, 100, 100));
    assert_eq!(buf.groups[1].quads, 1..3);

    assert!(buf.groups[2].scissor.is_none());
    assert_eq!(buf.groups[2].quads, 3..4);
}

#[test]
fn compose_intersects_nested_clips() {
    let buf = run(
        &[
            RenderCmd::PushClip(rect(0.0, 0.0, 100.0, 100.0)),
            RenderCmd::PushClip(rect(50.0, 50.0, 100.0, 100.0)),
            draw(rect(60.0, 60.0, 10.0, 10.0)),
            RenderCmd::PopClip,
            RenderCmd::PopClip,
        ],
        &params(1.0, [400, 400]),
    );
    assert_eq!(buf.quads.len(), 1);
    assert_eq!(buf.groups.len(), 1);
    let s = buf.groups[0]
        .scissor
        .expect("nested clip group must have a scissor");
    assert_eq!((s.x, s.y, s.w, s.h), (50, 50, 50, 50));
}

#[test]
fn compose_skips_groups_with_no_quads() {
    let buf = run(
        &[
            RenderCmd::PushClip(rect(0.0, 0.0, 50.0, 50.0)),
            RenderCmd::PopClip,
        ],
        &params(1.0, [200, 200]),
    );
    assert!(buf.quads.is_empty());
    assert!(buf.groups.is_empty());
}

#[test]
fn compose_scales_rects_for_dpr() {
    let buf = run(
        &[draw(rect(10.0, 20.0, 30.0, 40.0))],
        &params(2.0, [400, 400]),
    );
    assert_eq!(buf.quads.len(), 1);
    let q = &buf.quads[0];
    assert_eq!(q.pos, [20.0, 40.0]);
    assert_eq!(q.size, [60.0, 80.0]);
}

#[test]
fn intersect_disjoint_yields_zero_size() {
    let a = URect {
        x: 0,
        y: 0,
        w: 10,
        h: 10,
    };
    let b = URect {
        x: 100,
        y: 100,
        w: 10,
        h: 10,
    };
    // The composer uses `URect::clamp_to` for child↔parent scissor
    // intersection — disjoint rects collapse to a zero-sized result.
    let r = b.clamp_to(a);
    assert_eq!(r.w, 0);
    assert_eq!(r.h, 0);
}

#[test]
fn compose_translates_under_push_transform() {
    use crate::primitives::transform::TranslateScale;
    let buf = run(
        &[
            RenderCmd::PushTransform(TranslateScale::from_translation(glam::Vec2::new(
                100.0, 50.0,
            ))),
            draw(rect(10.0, 20.0, 30.0, 40.0)),
            RenderCmd::PopTransform,
        ],
        &params(1.0, [400, 400]),
    );
    assert_eq!(buf.quads.len(), 1);
    let q = &buf.quads[0];
    assert_eq!(q.pos, [110.0, 70.0]);
    assert_eq!(q.size, [30.0, 40.0]);
}

#[test]
fn compose_scales_radius_and_stroke_under_transform() {
    use crate::primitives::{stroke::Stroke, transform::TranslateScale};
    let buf = run(
        &[
            RenderCmd::PushTransform(TranslateScale::from_scale(2.0)),
            RenderCmd::DrawRectStroked(DrawRectStrokedPayload {
                rect: rect(0.0, 0.0, 50.0, 50.0),
                radius: Corners::all(8.0),
                fill: Color::rgb(1.0, 1.0, 1.0),
                stroke: Stroke {
                    width: 1.5,
                    color: Color::rgb(0.0, 0.0, 0.0),
                },
            }),
            RenderCmd::PopTransform,
        ],
        &params(1.0, [400, 400]),
    );
    let q = &buf.quads[0];
    assert_eq!(q.size, [100.0, 100.0]);
    assert_eq!(q.radius[0], 16.0);
    assert_eq!(q.stroke_width, 3.0);
}

#[test]
fn compose_composes_nested_transforms() {
    use crate::primitives::transform::TranslateScale;
    let buf = run(
        &[
            RenderCmd::PushTransform(TranslateScale::from_scale(2.0)),
            RenderCmd::PushTransform(TranslateScale::from_translation(glam::Vec2::new(10.0, 0.0))),
            draw(rect(5.0, 0.0, 10.0, 10.0)),
            RenderCmd::PopTransform,
            RenderCmd::PopTransform,
        ],
        &params(1.0, [400, 400]),
    );
    let q = &buf.quads[0];
    assert_eq!(q.pos, [30.0, 0.0]);
    assert_eq!(q.size, [20.0, 20.0]);
}

#[test]
fn compose_transforms_clip_rects_to_screen_space() {
    use crate::primitives::transform::TranslateScale;
    let buf = run(
        &[
            RenderCmd::PushTransform(TranslateScale::from_scale(2.0)),
            RenderCmd::PushClip(rect(10.0, 10.0, 20.0, 20.0)),
            draw(rect(15.0, 15.0, 5.0, 5.0)),
            RenderCmd::PopClip,
            RenderCmd::PopTransform,
        ],
        &params(1.0, [400, 400]),
    );
    assert_eq!(buf.groups.len(), 1);
    let s = buf.groups[0]
        .scissor
        .expect("clipped group must have a scissor");
    assert_eq!((s.x, s.y, s.w, s.h), (20, 20, 40, 40));
}

/// Pin: a `Quad → Text → Quad` paint sequence inside a single scissor
/// produces TWO groups so the second quad renders *after* the text.
/// Without this split, `submit` batches both quads together and the
/// text always paints on top — which is the bug the `text z-order`
/// showcase tab exposes.
#[test]
fn compose_splits_group_on_text_to_quad_transition() {
    let buf = run(
        &[
            draw(rect(0.0, 0.0, 100.0, 100.0)),
            text(rect(10.0, 10.0, 80.0, 20.0)),
            draw(rect(20.0, 20.0, 60.0, 40.0)),
        ],
        &params(1.0, [200, 200]),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.texts.len(), 1);
    assert_eq!(
        buf.groups.len(),
        2,
        "text→quad transition must start a new group"
    );
    // First group: quad #0 + text #0.
    assert_eq!(buf.groups[0].quads, 0..1);
    assert_eq!(buf.groups[0].texts, 0..1);
    // Second group: quad #1 only — renders after group 0's text.
    assert_eq!(buf.groups[1].quads, 1..2);
    assert_eq!(buf.groups[1].texts, 1..1);
}

/// Pin: consecutive `Text → Text` should NOT split (both go into the
/// same group). Only `Text → Quad` triggers a flush. Otherwise a
/// header-then-body label pair produces two groups for nothing.
#[test]
fn compose_does_not_split_consecutive_texts() {
    let buf = run(
        &[
            draw(rect(0.0, 0.0, 100.0, 100.0)),
            text(rect(10.0, 10.0, 80.0, 20.0)),
            text(rect(10.0, 35.0, 80.0, 20.0)),
        ],
        &params(1.0, [200, 200]),
    );
    assert_eq!(buf.quads.len(), 1);
    assert_eq!(buf.texts.len(), 2);
    assert_eq!(buf.groups.len(), 1);
    assert_eq!(buf.groups[0].quads, 0..1);
    assert_eq!(buf.groups[0].texts, 0..2);
}

/// Pin: `Quad → Quad → Text` fits in one group. The text comes after
/// both quads and renders on top of both — the common case (button
/// background + button stroke + label).
#[test]
fn compose_keeps_quads_then_text_in_one_group() {
    let buf = run(
        &[
            draw(rect(0.0, 0.0, 100.0, 100.0)),
            draw(rect(2.0, 2.0, 96.0, 96.0)),
            text(rect(10.0, 10.0, 80.0, 20.0)),
        ],
        &params(1.0, [200, 200]),
    );
    assert_eq!(buf.groups.len(), 1);
    assert_eq!(buf.groups[0].quads, 0..2);
    assert_eq!(buf.groups[0].texts, 0..1);
}
