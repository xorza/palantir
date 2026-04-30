//! Tests for the wgpu-free portion of the backend (`process`, scissor math).
//! GPU-touching code (`Renderer::render`) is exercised by example/manual runs.

use super::*;
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

#[test]
fn process_with_no_clip_emits_one_unscissored_group() {
    let cmds = vec![
        draw(rect(0.0, 0.0, 10.0, 10.0)),
        draw(rect(20.0, 0.0, 10.0, 10.0)),
    ];
    let mut quads = Vec::new();
    let mut groups = Vec::new();
    process(&cmds, 1.0, false, [200, 200], &mut quads, &mut groups);

    assert_eq!(quads.len(), 2);
    assert_eq!(groups.len(), 1);
    assert!(groups[0].scissor.is_none());
    assert_eq!(groups[0].start, 0);
    assert_eq!(groups[0].end, 2);
}

#[test]
fn process_with_clip_groups_inner_draws_under_scissor() {
    let cmds = vec![
        draw(rect(0.0, 0.0, 10.0, 10.0)),
        RenderCmd::PushClip(rect(50.0, 50.0, 100.0, 100.0)),
        draw(rect(60.0, 60.0, 20.0, 20.0)),
        draw(rect(90.0, 90.0, 20.0, 20.0)),
        RenderCmd::PopClip,
        draw(rect(0.0, 0.0, 5.0, 5.0)),
    ];
    let mut quads = Vec::new();
    let mut groups = Vec::new();
    process(&cmds, 1.0, false, [400, 400], &mut quads, &mut groups);

    assert_eq!(quads.len(), 4);
    assert_eq!(groups.len(), 3);

    // Group 0: pre-clip draw, no scissor.
    assert!(groups[0].scissor.is_none());
    assert_eq!(groups[0].start..groups[0].end, 0..1);

    // Group 1: clipped draws, with scissor at the pushed rect.
    let s = groups[1]
        .scissor
        .expect("clipped group must have a scissor");
    assert_eq!((s.x, s.y, s.w, s.h), (50, 50, 100, 100));
    assert_eq!(groups[1].start..groups[1].end, 1..3);

    // Group 2: post-pop draw, back to no scissor.
    assert!(groups[2].scissor.is_none());
    assert_eq!(groups[2].start..groups[2].end, 3..4);
}

#[test]
fn process_intersects_nested_clips() {
    let cmds = vec![
        RenderCmd::PushClip(rect(0.0, 0.0, 100.0, 100.0)),
        RenderCmd::PushClip(rect(50.0, 50.0, 100.0, 100.0)),
        draw(rect(60.0, 60.0, 10.0, 10.0)),
        RenderCmd::PopClip,
        RenderCmd::PopClip,
    ];
    let mut quads = Vec::new();
    let mut groups = Vec::new();
    process(&cmds, 1.0, false, [400, 400], &mut quads, &mut groups);

    assert_eq!(quads.len(), 1);
    assert_eq!(groups.len(), 1);
    let s = groups[0]
        .scissor
        .expect("nested clip group must have a scissor");
    // Intersection of (0..100, 0..100) and (50..150, 50..150) = (50..100, 50..100).
    assert_eq!((s.x, s.y, s.w, s.h), (50, 50, 50, 50));
}

#[test]
fn process_skips_groups_with_no_quads() {
    // Push then pop without drawing anything — no group should be emitted for
    // either segment (current_start == end at every transition).
    let cmds = vec![
        RenderCmd::PushClip(rect(0.0, 0.0, 50.0, 50.0)),
        RenderCmd::PopClip,
    ];
    let mut quads = Vec::new();
    let mut groups = Vec::new();
    process(&cmds, 1.0, false, [200, 200], &mut quads, &mut groups);

    assert!(quads.is_empty());
    assert!(groups.is_empty());
}

#[test]
fn process_scales_rects_for_dpr() {
    let cmds = vec![draw(rect(10.0, 20.0, 30.0, 40.0))];
    let mut quads = Vec::new();
    let mut groups = Vec::new();
    process(&cmds, 2.0, false, [400, 400], &mut quads, &mut groups);

    assert_eq!(quads.len(), 1);
    let q = &quads[0];
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
