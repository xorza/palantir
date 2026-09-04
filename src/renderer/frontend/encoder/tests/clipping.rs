//! Push/pop balance, and when a rounded clip needs the stencil.

use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::shadow::Shadow;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::RgbaF32, stroke::Stroke};
use crate::renderer::frontend::capture::PaintCall;
use crate::renderer::frontend::capture::PaintCapture;
use crate::renderer::frontend::encoder::tests::support::{as_rect, count_draw_rects};
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

/// Pin: a clip-only Surface (no painted background) still emits a
/// PushClip/PopClip pair so children get clipped, while contributing zero
/// rect quads of its own.
#[test]
fn clip_only_surface_emits_clip_but_no_draw() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("clip_only"))
                .size(50.0)
                .clip_rect()
                .show(ui, |_| {});
        });
    });
    let cmds = h.encode_paint();
    let ClipPairs { pushes, pops } = count_clip_pairs(&cmds);
    assert_eq!(pushes, 1);
    assert_eq!(pops, 1);
    assert_eq!(count_draw_rects(&cmds), 0);
}

#[test]
fn clip_emits_balanced_push_pop() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("clip"))
                .size(50.0)
                .clip_rect()
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("inner"))
                        .size(40.0)
                        .background(Background {
                            fill: RgbaF32::srgb(0.5, 0.5, 0.5).into(),
                            ..Default::default()
                        })
                        .show(ui);
                });
        });
    });
    let cmds = h.encode_paint();

    let ClipPairs { pushes, pops } = count_clip_pairs(&cmds);
    assert_eq!(pushes, 1);
    assert_eq!(pops, 1);

    let push_idx = cmds
        .calls
        .iter()
        .position(|command| matches!(command, PaintCall::PushClip(_)))
        .unwrap();
    let pop_idx = cmds
        .calls
        .iter()
        .position(|command| matches!(command, PaintCall::PopClip))
        .unwrap();
    let draw_idxs: Vec<_> = cmds
        .calls
        .iter()
        .enumerate()
        .filter_map(|(i, command)| as_rect(command).map(|_| i))
        .collect();
    assert!(!draw_idxs.is_empty());
    for &di in &draw_idxs {
        assert!(
            di > push_idx && di < pop_idx,
            "draw at {di} not inside [{push_idx}, {pop_idx}]"
        );
    }
}

/// Rounded-clip emission, plus encoded mask geometry: with zero padding
/// the mask is inset by the chrome's stroke width (folded into padding at
/// `open_node`) so children can't overpaint the stroke ring.
#[test]
fn clip_rounded_emits_push_clip_rounded_when_background_has_radius() {
    use crate::primitives::corners::Corners;
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("rounded"))
                .size(80.0)
                .background(Background {
                    fill: RgbaF32::srgb(0.2, 0.2, 0.2).into(),
                    stroke: Stroke::solid(RgbaF32::srgb(1.0, 1.0, 1.0), 2.0),
                    corners: Corners::all(8.0),
                    shadow: Shadow::NONE,
                })
                .clip_rounded()
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("c"))
                        .size(40.0)
                        .show(ui);
                });
        });
    });
    let cmds = h.encode_paint();

    let rounded_clips: Vec<_> = cmds
        .calls
        .iter()
        .filter_map(|command| match command {
            PaintCall::PushClip(payload) if !payload.corners.approx_zero() => Some(payload),
            _ => None,
        })
        .collect();
    assert_eq!(rounded_clips.len(), 1);
    let payload = rounded_clips[0];

    let panel_rect = h
        .layout_rect(WidgetId::from_hash("rounded"))
        .expect("arranged");
    // Stroke=2 is auto-folded into padding by `Tree::open_node`, so the
    // encoder's `rect.deflated_by(padding)` insets the mask by 2 on
    // every side. Radius reduces by 2 to stay concentric with the
    // painted stroke's inner edge.
    assert_eq!(payload.rect, panel_rect.deflated_by(Spacing::all(2.0)));
    assert_eq!(payload.corners, Corners::all(6.0));
}

#[test]
fn clip_rounded_falls_back_to_scissor_without_background() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("rounded_no_bg"))
                .size(80.0)
                .clip_rounded()
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("c"))
                        .size(40.0)
                        .show(ui);
                });
        });
    });
    let cmds = h.encode_paint();
    let push_clips: Vec<PushClipPayload> = cmds
        .calls
        .iter()
        .filter_map(|command| match command {
            PaintCall::PushClip(payload) => Some(*payload),
            _ => None,
        })
        .collect();
    assert_eq!(push_clips.len(), 1);
    assert!(
        push_clips[0].corners.approx_zero(),
        "no background → no radius → falls back to plain scissor",
    );
}

#[test]
fn nested_clips_each_emit_their_own_pair() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("outer"))
                .size(Sizing::fixed(100.0))
                .clip_rect()
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("inner"))
                        .size(Sizing::fixed(50.0))
                        .clip_rect()
                        .show(ui, |_| {});
                });
        });
    });
    let cmds = h.encode_paint();
    let ClipPairs { pushes, pops } = count_clip_pairs(&cmds);
    assert_eq!(pushes, 2);
    assert_eq!(pops, 2);
}

#[derive(Debug)]
struct ClipPairs {
    pushes: usize,
    pops: usize,
}

fn count_clip_pairs(cmds: &PaintCapture) -> ClipPairs {
    let pushes = cmds
        .calls
        .iter()
        .filter(|command| matches!(command, PaintCall::PushClip(_)))
        .count();
    let pops = cmds
        .calls
        .iter()
        .filter(|command| matches!(command, PaintCall::PopClip))
        .count();
    ClipPairs { pushes, pops }
}
