use super::super::buffer::{RenderBuffer, ScissorRect};
use super::super::encoder::RenderCmd;
use super::{ComposeParams, compose, intersect_scissor};
use crate::primitives::{Color, Corners, Rect};

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y, w, h)
}

fn draw(r: Rect) -> RenderCmd {
    RenderCmd::DrawRect {
        rect: r,
        radius: Corners::ZERO,
        fill: Color::rgb(1.0, 1.0, 1.0),
        stroke: None,
    }
}

fn params(scale: f32, viewport_phys: [u32; 2]) -> ComposeParams {
    ComposeParams {
        viewport_logical: [
            viewport_phys[0] as f32 / scale,
            viewport_phys[1] as f32 / scale,
        ],
        scale,
        pixel_snap: false,
    }
}

#[test]
fn compose_with_no_clip_emits_one_unscissored_group() {
    let cmds = vec![
        draw(rect(0.0, 0.0, 10.0, 10.0)),
        draw(rect(20.0, 0.0, 10.0, 10.0)),
    ];
    let mut out = RenderBuffer::new();
    compose(&cmds, &params(1.0, [200, 200]), &mut out);

    assert_eq!(out.quads.len(), 2);
    assert_eq!(out.groups.len(), 1);
    assert!(out.groups[0].scissor.is_none());
    assert_eq!(out.groups[0].start, 0);
    assert_eq!(out.groups[0].end, 2);
}

#[test]
fn compose_with_clip_groups_inner_draws_under_scissor() {
    let cmds = vec![
        draw(rect(0.0, 0.0, 10.0, 10.0)),
        RenderCmd::PushClip(rect(50.0, 50.0, 100.0, 100.0)),
        draw(rect(60.0, 60.0, 20.0, 20.0)),
        draw(rect(90.0, 90.0, 20.0, 20.0)),
        RenderCmd::PopClip,
        draw(rect(0.0, 0.0, 5.0, 5.0)),
    ];
    let mut out = RenderBuffer::new();
    compose(&cmds, &params(1.0, [400, 400]), &mut out);

    assert_eq!(out.quads.len(), 4);
    assert_eq!(out.groups.len(), 3);

    assert!(out.groups[0].scissor.is_none());
    assert_eq!(out.groups[0].start..out.groups[0].end, 0..1);

    let s = out.groups[1]
        .scissor
        .expect("clipped group must have a scissor");
    assert_eq!((s.x, s.y, s.w, s.h), (50, 50, 100, 100));
    assert_eq!(out.groups[1].start..out.groups[1].end, 1..3);

    assert!(out.groups[2].scissor.is_none());
    assert_eq!(out.groups[2].start..out.groups[2].end, 3..4);
}

#[test]
fn compose_intersects_nested_clips() {
    let cmds = vec![
        RenderCmd::PushClip(rect(0.0, 0.0, 100.0, 100.0)),
        RenderCmd::PushClip(rect(50.0, 50.0, 100.0, 100.0)),
        draw(rect(60.0, 60.0, 10.0, 10.0)),
        RenderCmd::PopClip,
        RenderCmd::PopClip,
    ];
    let mut out = RenderBuffer::new();
    compose(&cmds, &params(1.0, [400, 400]), &mut out);

    assert_eq!(out.quads.len(), 1);
    assert_eq!(out.groups.len(), 1);
    let s = out.groups[0]
        .scissor
        .expect("nested clip group must have a scissor");
    assert_eq!((s.x, s.y, s.w, s.h), (50, 50, 50, 50));
}

#[test]
fn compose_skips_groups_with_no_quads() {
    let cmds = vec![
        RenderCmd::PushClip(rect(0.0, 0.0, 50.0, 50.0)),
        RenderCmd::PopClip,
    ];
    let mut out = RenderBuffer::new();
    compose(&cmds, &params(1.0, [200, 200]), &mut out);

    assert!(out.quads.is_empty());
    assert!(out.groups.is_empty());
}

#[test]
fn compose_scales_rects_for_dpr() {
    let cmds = vec![draw(rect(10.0, 20.0, 30.0, 40.0))];
    let mut out = RenderBuffer::new();
    compose(&cmds, &params(2.0, [400, 400]), &mut out);

    assert_eq!(out.quads.len(), 1);
    let q = &out.quads[0];
    assert_eq!(q.pos, [20.0, 40.0]);
    assert_eq!(q.size, [60.0, 80.0]);
}

#[test]
fn intersect_disjoint_yields_zero_size() {
    let a = ScissorRect {
        x: 0,
        y: 0,
        w: 10,
        h: 10,
    };
    let b = ScissorRect {
        x: 100,
        y: 100,
        w: 10,
        h: 10,
    };
    let r = intersect_scissor(a, b);
    assert_eq!(r.w, 0);
    assert_eq!(r.h, 0);
}

#[test]
fn compose_translates_under_push_transform() {
    use crate::primitives::TranslateScale;
    let cmds = vec![
        RenderCmd::PushTransform(TranslateScale::from_translation(glam::Vec2::new(
            100.0, 50.0,
        ))),
        draw(rect(10.0, 20.0, 30.0, 40.0)),
        RenderCmd::PopTransform,
    ];
    let mut out = RenderBuffer::new();
    compose(&cmds, &params(1.0, [400, 400]), &mut out);
    assert_eq!(out.quads.len(), 1);
    let q = &out.quads[0];
    assert_eq!(q.pos, [110.0, 70.0]);
    assert_eq!(q.size, [30.0, 40.0]);
}

#[test]
fn compose_scales_radius_and_stroke_under_transform() {
    use crate::primitives::{Stroke, TranslateScale};
    let cmds = vec![
        RenderCmd::PushTransform(TranslateScale::from_scale(2.0)),
        RenderCmd::DrawRect {
            rect: rect(0.0, 0.0, 50.0, 50.0),
            radius: Corners::all(8.0),
            fill: Color::rgb(1.0, 1.0, 1.0),
            stroke: Some(Stroke {
                width: 1.5,
                color: Color::rgb(0.0, 0.0, 0.0),
            }),
        },
        RenderCmd::PopTransform,
    ];
    let mut out = RenderBuffer::new();
    compose(&cmds, &params(1.0, [400, 400]), &mut out);
    let q = &out.quads[0];
    assert_eq!(q.size, [100.0, 100.0]);
    assert_eq!(q.radius[0], 16.0);
    assert_eq!(q.stroke_width, 3.0);
}

#[test]
fn compose_composes_nested_transforms() {
    use crate::primitives::TranslateScale;
    let cmds = vec![
        RenderCmd::PushTransform(TranslateScale::from_scale(2.0)),
        RenderCmd::PushTransform(TranslateScale::from_translation(glam::Vec2::new(10.0, 0.0))),
        draw(rect(5.0, 0.0, 10.0, 10.0)),
        RenderCmd::PopTransform,
        RenderCmd::PopTransform,
    ];
    let mut out = RenderBuffer::new();
    compose(&cmds, &params(1.0, [400, 400]), &mut out);
    let q = &out.quads[0];
    assert_eq!(q.pos, [30.0, 0.0]);
    assert_eq!(q.size, [20.0, 20.0]);
}

#[test]
fn compose_transforms_clip_rects_to_screen_space() {
    use crate::primitives::TranslateScale;
    let cmds = vec![
        RenderCmd::PushTransform(TranslateScale::from_scale(2.0)),
        RenderCmd::PushClip(rect(10.0, 10.0, 20.0, 20.0)),
        draw(rect(15.0, 15.0, 5.0, 5.0)),
        RenderCmd::PopClip,
        RenderCmd::PopTransform,
    ];
    let mut out = RenderBuffer::new();
    compose(&cmds, &params(1.0, [400, 400]), &mut out);
    assert_eq!(out.groups.len(), 1);
    let s = out.groups[0]
        .scissor
        .expect("clipped group must have a scissor");
    assert_eq!((s.x, s.y, s.w, s.h), (20, 20, 40, 40));
}
