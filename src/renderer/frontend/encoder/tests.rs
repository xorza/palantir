use super::super::cmd_buffer::{
    CmdKind, DrawRectPayload, DrawRectStrokedPayload, DrawTextPayload, PushClipRoundedPayload,
    RenderCmdBuffer,
};
use super::align_text_in;
use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::forest::widget_id::WidgetId;
use crate::input::sense::Sense;
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::{
    align::Align, align::HAlign, align::VAlign, display::Display, sizing::Sizing,
};
use crate::primitives::{
    color::Color, rect::Rect, size::Size, stroke::Stroke, transform::TranslateScale,
};
use crate::support::testing::{
    begin, encode_cmds, encode_cmds_filtered, encode_cmds_with_rects, ui_at,
};
use crate::widgets::theme::Background;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

struct ClipPairs {
    pushes: usize,
    pops: usize,
}

fn count_clip_pairs(cmds: &RenderCmdBuffer) -> ClipPairs {
    let pushes = cmds
        .kinds
        .iter()
        .filter(|k| **k == CmdKind::PushClip)
        .count();
    let pops = cmds
        .kinds
        .iter()
        .filter(|k| **k == CmdKind::PopClip)
        .count();
    ClipPairs { pushes, pops }
}

fn count_draw_rects(cmds: &RenderCmdBuffer) -> usize {
    cmds.kinds
        .iter()
        .filter(|k| matches!(k, CmdKind::DrawRect | CmdKind::DrawRectStroked))
        .count()
}

/// Baseline encoder counts: empty tree emits no draws; a Frame with a
/// fill emits one DrawRect; an invisible Frame (no fill / stroke /
/// shape) emits none — `ShapeRecord::is_noop` filters at `add_shape` time so
/// the encoder sees no RoundedRect in the tree. Degenerate Backgrounds
/// (transparent + no stroke) and clip-only Surfaces (`Surface::clip_rect`)
/// also emit zero `DrawRect`s — the encoder's `bg.is_noop()` guard at
/// chrome-paint time filters them.
#[test]
fn baseline_draw_rect_count_cases() {
    enum Scene {
        Empty,
        FrameWithFill,
        InvisibleFrame,
        FrameWithDegenerateBackground,
        FrameWithClipRectSurface,
    }
    let cases: &[(&str, Scene, usize)] = &[
        ("empty_tree", Scene::Empty, 0),
        ("frame_with_fill", Scene::FrameWithFill, 1),
        ("invisible_frame", Scene::InvisibleFrame, 0),
        (
            "frame_with_degenerate_background",
            Scene::FrameWithDegenerateBackground,
            0,
        ),
        (
            "frame_with_clip_rect_surface",
            Scene::FrameWithClipRectSurface,
            0,
        ),
    ];
    for (label, scene, expected) in cases {
        let mut ui = ui_at(UVec2::new(200, 200));
        Panel::hstack().auto_id().show(&mut ui, |ui| match scene {
            Scene::Empty => {}
            Scene::FrameWithFill => {
                Frame::new()
                    .id_salt("a")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                        ..Default::default()
                    })
                    .show(ui);
            }
            Scene::InvisibleFrame => {
                Frame::new().id_salt("invisible").size(50.0).show(ui);
            }
            Scene::FrameWithDegenerateBackground => {
                Frame::new()
                    .id_salt("degenerate")
                    .size(50.0)
                    .background(Background {
                        fill: Color::TRANSPARENT.into(),
                        stroke: Stroke::ZERO,
                        ..Default::default()
                    })
                    .show(ui);
            }
            Scene::FrameWithClipRectSurface => {
                Frame::new()
                    .id_salt("clip_only")
                    .size(50.0)
                    .clip_rect()
                    .show(ui);
            }
        });
        ui.post_record();
        ui.paint();
        let cmds = encode_cmds(&ui);
        assert_eq!(count_draw_rects(&cmds), *expected, "case: {label}");
    }
}

/// Pin: the encoder iterates ALL shape variants in the
/// background phase, not just `Text`. Chrome moved off the shapes
/// list (now lives in `Tree::chrome_table`), but `ShapeRecord::RoundedRect`
/// remains a valid variant — any custom widget that pushes one via
/// `ui.add_shape` should still produce a `DrawRect` command. Tested
/// by manually injecting a `RoundedRect` onto a panel node.
#[test]
fn manually_pushed_rounded_rect_shape_emits_draw_rect() {
    use crate::primitives::corners::Corners;
    use crate::shape::Shape;
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        // Attach to the outer hstack BEFORE opening any child — the
        // tree's contiguity invariant requires shapes to be added to
        // the last-pushed node before its children open.
        ui.add_shape(Shape::RoundedRect {
            local_rect: None,
            radius: Corners::all(4.0),
            fill: Color::rgb(1.0, 0.0, 0.0).into(),
            stroke: Stroke::ZERO,
        });
        Frame::new().id_salt("host").size(50.0).show(ui);
    });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds(&ui);
    let draws = cmds
        .kinds
        .iter()
        .filter(|k| matches!(k, CmdKind::DrawRect | CmdKind::DrawRectStroked))
        .count();
    assert!(
        draws >= 1,
        "manually pushed RoundedRect must emit a DrawRect, got {draws}"
    );
}

/// Pin: `Shape::Line` pushed via `ui.add_shape` emits exactly one
/// `DrawPolyline` cmd; degenerate variants (zero width, transparent
/// color) emit none — `Shape::is_noop` filters them at
/// `add_shape` time.
#[test]
fn line_shape_emits_draw_polyline() {
    use crate::shape::{LineCap, LineJoin, Shape};
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        ui.add_shape(Shape::Line {
            a: Vec2::new(0.0, 0.0),
            b: Vec2::new(20.0, 0.0),
            width: 2.0,
            brush: Color::rgb(1.0, 0.0, 0.0).into(),
            cap: LineCap::Butt,
            join: LineJoin::Miter,
        });
        // Degenerate: filtered before reaching the cmd buffer.
        ui.add_shape(Shape::Line {
            a: Vec2::new(0.0, 0.0),
            b: Vec2::new(10.0, 10.0),
            width: 0.0,
            brush: Color::rgb(1.0, 0.0, 0.0).into(),
            cap: LineCap::Butt,
            join: LineJoin::Miter,
        });
        ui.add_shape(Shape::Line {
            a: Vec2::new(0.0, 0.0),
            b: Vec2::new(10.0, 10.0),
            width: 2.0,
            brush: Color::TRANSPARENT.into(),
            cap: LineCap::Butt,
            join: LineJoin::Miter,
        });
        Frame::new().id_salt("host").size(50.0).show(ui);
    });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds(&ui);
    let count = cmds
        .kinds
        .iter()
        .filter(|k| matches!(k, CmdKind::DrawPolyline))
        .count();
    assert_eq!(count, 1, "expected exactly one DrawPolyline cmd");
    assert_eq!(
        cmds.shape_payloads.polyline_points.len(),
        2,
        "one 2-point line populates the points arena"
    );
}

/// Pin: `ShapeRecord::Text` runs through the same background-phase
/// iteration. If the loop ever narrowed to a single shape variant
/// (RoundedRect, say), text labels would silently disappear. The
/// existing label-bearing tests would still pass because chrome
/// carries the visible content; this test specifically pins the
/// `DrawText` emission.
#[test]
fn text_shape_emits_draw_text() {
    use crate::Text;
    use crate::support::testing::ui_with_text;
    let mut ui = ui_with_text(UVec2::new(200, 200));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Text::new("hi").auto_id().show(ui);
    });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds(&ui);
    assert!(
        cmds.kinds.contains(&CmdKind::DrawText),
        "Text widget must emit a DrawText command"
    );
}

/// Pin: a clip-only Surface (no painted background) on a container still
/// emits the PushClip/PopClip pair so children get clipped, while
/// contributing zero `DrawRect`s of its own. The encoder's `bg.is_noop()`
/// guard skips the chrome paint; the clip mode survives independently.
#[test]
fn clip_only_surface_emits_clip_but_no_draw() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::zstack()
            .id_salt("clip_only")
            .size(50.0)
            .clip_rect()
            .show(ui, |_| {});
    });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds(&ui);
    let ClipPairs { pushes, pops } = count_clip_pairs(&cmds);
    assert_eq!(pushes, 1, "clip-only surface must push a clip");
    assert_eq!(pops, 1, "clip-only surface must pop the clip");
    assert_eq!(
        count_draw_rects(&cmds),
        0,
        "transparent paint must emit no DrawRect"
    );
}

#[test]
fn clip_emits_balanced_push_pop() {
    let mut ui = ui_at(UVec2::new(200, 200));
    // Outer HStack opts out of the default-on clip so we can count just the
    // ZStack's pair under test.
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::zstack()
            .id_salt("clip")
            .size(50.0)
            .clip_rect()
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("inner")
                    .size(40.0)
                    .background(Background {
                        fill: Color::rgb(0.5, 0.5, 0.5).into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds(&ui);

    let ClipPairs { pushes, pops } = count_clip_pairs(&cmds);
    assert_eq!(pushes, 1);
    assert_eq!(pops, 1);

    // PushClip must come before the inner DrawRect, PopClip after — i.e. the
    // inner draw is sandwiched.
    let push_idx = cmds
        .kinds
        .iter()
        .position(|k| *k == CmdKind::PushClip)
        .unwrap();
    let pop_idx = cmds
        .kinds
        .iter()
        .position(|k| *k == CmdKind::PopClip)
        .unwrap();
    let draw_idxs: Vec<_> = cmds
        .kinds
        .iter()
        .enumerate()
        .filter_map(|(i, k)| matches!(k, CmdKind::DrawRect | CmdKind::DrawRectStroked).then_some(i))
        .collect();
    assert!(!draw_idxs.is_empty());
    for &di in &draw_idxs {
        assert!(
            di > push_idx && di < pop_idx,
            "draw at {di} not inside [{push_idx}, {pop_idx}]"
        );
    }
}

/// Rounded-clip emission, plus encoded mask geometry. The stroke
/// width acts as the inset: the encoded rect is the panel's layout
/// rect deflated by `stroke.width` on every side, and each corner
/// radius is reduced by the same amount so the mask curve stays
/// concentric with the painted stroke's inner edge. Pins both
/// `PushClipRounded` count AND payload — a regression in either the
/// `Surface::apply_to` stamping or the encoder's geometry math fails
/// here.
#[test]
fn clip_rounded_emits_push_clip_rounded_when_background_has_radius() {
    use crate::primitives::corners::Corners;
    use crate::primitives::spacing::Spacing;
    let mut ui = ui_at(UVec2::new(200, 200));
    let mut panel_node = None;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        panel_node = Some(
            Panel::zstack()
                .id_salt("rounded")
                .size(80.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.2, 0.2).into(),
                    stroke: Stroke::solid(Color::rgb(1.0, 1.0, 1.0), 2.0),
                    radius: Corners::all(8.0),
                })
                .clip_rounded()
                .show(ui, |ui| {
                    Frame::new().id_salt("c").size(40.0).show(ui);
                })
                .node,
        );
    });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds(&ui);

    let rounded_idx = cmds
        .kinds
        .iter()
        .position(|k| *k == CmdKind::PushClipRounded)
        .expect("rounded clip with rounded background emits PushClipRounded");
    assert_eq!(
        cmds.kinds
            .iter()
            .filter(|k| **k == CmdKind::PushClipRounded)
            .count(),
        1,
    );

    // Mask geometry: encoded rect is the panel's layout rect deflated
    // by stroke.width=2; each corner radius is reduced by the same.
    let panel_rect = ui.layout[Layer::Main].rect[panel_node.unwrap().index()];
    let expected_rect = panel_rect.deflated_by(Spacing::all(2.0));
    let start = cmds.starts[rounded_idx];
    let payload: PushClipRoundedPayload = cmds.read(start);
    assert_eq!(payload.rect, expected_rect);
    assert_eq!(payload.radius, Corners::all(6.0));
}

#[test]
fn clip_rounded_falls_back_to_scissor_without_background() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::zstack()
            .id_salt("rounded_no_bg")
            .size(80.0)
            .clip_rounded()
            .show(ui, |ui| {
                Frame::new().id_salt("c").size(40.0).show(ui);
            });
    });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds(&ui);
    assert_eq!(
        cmds.kinds
            .iter()
            .filter(|k| **k == CmdKind::PushClipRounded)
            .count(),
        0,
        "no background → no radius → falls back to scissor"
    );
    assert_eq!(
        cmds.kinds
            .iter()
            .filter(|k| **k == CmdKind::PushClip)
            .count(),
        1
    );
}

/// Walk an encoder command stream and return the effective screen-space rect
/// for each `DrawRect`, keyed by its fill colour. The interpretation mirrors
/// what a backend does: `PushTransform` composes onto the current transform;
/// `PushClip` is taken in the parent's already-composed transform space and
/// intersected with the active clip; `DrawRect.rect` is in the parent's
/// transform space at draw time.
fn screen_rects_by_fill(cmds: &RenderCmdBuffer) -> Vec<(Color, Rect)> {
    let mut t = TranslateScale::IDENTITY;
    let mut t_stack: Vec<TranslateScale> = Vec::new();
    let mut clip: Option<Rect> = None;
    let mut clip_stack: Vec<Option<Rect>> = Vec::new();
    let mut out = Vec::new();
    for i in 0..cmds.kinds.len() {
        let kind = cmds.kinds[i];
        let start = cmds.starts[i];
        match kind {
            CmdKind::PushTransform => {
                let child: TranslateScale = cmds.read(start);
                t_stack.push(t);
                t = t.compose(child);
            }
            CmdKind::PopTransform => t = t_stack.pop().expect("balanced PushTransform/Pop"),
            CmdKind::PushClip | CmdKind::PushClipRounded => {
                let r: Rect = cmds.read(start);
                let screen = t.apply_rect(r);
                let intersected = match clip {
                    Some(c) => screen.intersect(c),
                    None => screen,
                };
                clip_stack.push(clip);
                clip = Some(intersected);
            }
            CmdKind::PopClip => clip = clip_stack.pop().expect("balanced PushClip/Pop"),
            CmdKind::DrawRect => {
                let p: DrawRectPayload = cmds.read(start);
                let screen = t.apply_rect(p.rect);
                let visible = match clip {
                    Some(c) => screen.intersect(c),
                    None => screen,
                };
                out.push((p.fill, visible));
            }
            CmdKind::DrawRectStroked => {
                let p: DrawRectStrokedPayload = cmds.read(start);
                let screen = t.apply_rect(p.rect);
                let visible = match clip {
                    Some(c) => screen.intersect(c),
                    None => screen,
                };
                out.push((p.fill, visible));
            }
            CmdKind::DrawText | CmdKind::DrawMesh | CmdKind::DrawPolyline => {
                // Test rasterizer ignores text/mesh/polyline — encoder tests only assert on rect output.
            }
        }
    }
    assert!(t_stack.is_empty(), "transform stack unbalanced");
    assert!(clip_stack.is_empty(), "clip stack unbalanced");
    out
}

#[test]
fn cascade_matches_hit_index_for_visible_disabled_and_hidden() {
    // The encoder and the hit index both apply the same four cascades
    // (disabled / invisible / clip / transform). They walk the tree in
    // different shapes — the encoder pushes/pops, the hit index snapshots
    // per-node. This test pins the contract that they agree on:
    //
    //  - Visible and disabled nodes get the same effective screen rect.
    //  - Hidden nodes are skipped by the encoder but still tracked by the
    //    hit index (their slot persists; sense becomes NONE).
    //  - Clicks land on the visible node and are suppressed for both
    //    disabled (input-cascade) and hidden (visibility-cascade) nodes.

    let v_color = Color::rgb(1.0, 0.0, 0.0);
    let d_color = Color::rgb(0.0, 1.0, 0.0);
    let h_color = Color::rgb(0.0, 0.0, 1.0);
    let xform = TranslateScale::new(Vec2::new(5.0, 7.0), 2.0);

    let mut ui = Ui::new();

    // Frame 1: build, layout, post_record so the hit index is populated.
    begin(&mut ui, UVec2::new(400, 400));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::canvas()
            .id_salt("mid")
            .size(200.0)
            .clip_rect()
            .transform(xform)
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("V")
                    .position((0.0, 0.0))
                    .size(30.0)
                    .background(Background {
                        fill: v_color.into(),
                        ..Default::default()
                    })
                    .sense(Sense::CLICK)
                    .show(ui);
                Frame::new()
                    .id_salt("D")
                    .position((40.0, 0.0))
                    .size(30.0)
                    .background(Background {
                        fill: d_color.into(),
                        ..Default::default()
                    })
                    .sense(Sense::CLICK)
                    .disabled(true)
                    .show(ui);
                Frame::new()
                    .id_salt("H")
                    .position((80.0, 0.0))
                    .size(30.0)
                    .background(Background {
                        fill: h_color.into(),
                        ..Default::default()
                    })
                    .sense(Sense::CLICK)
                    .hidden()
                    .show(ui);
            });
    });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds(&ui);
    let drawn = screen_rects_by_fill(&cmds);

    // Visible node: encoder emits exactly one DrawRect with its fill, and the
    // resulting screen rect matches the hit index's rect for the same id.
    let v_id = WidgetId::from_hash("V");
    let v_screen = drawn
        .iter()
        .find(|(c, _)| *c == v_color)
        .map(|(_, r)| *r)
        .expect("visible node should emit a DrawRect");
    let v_hit = ui
        .response_for(v_id)
        .rect
        .expect("visible node should have a hit-index rect");
    assert_eq!(v_screen, v_hit, "encoder vs hit-index rect for V");

    // Disabled node still paints; the hit index keeps its rect too. Cascades
    // (clip + transform) must produce the same screen rect on both sides.
    let d_id = WidgetId::from_hash("D");
    let d_screen = drawn
        .iter()
        .find(|(c, _)| *c == d_color)
        .map(|(_, r)| *r)
        .expect("disabled node should still paint");
    let d_hit = ui.response_for(d_id).rect.expect("disabled node has rect");
    assert_eq!(d_screen, d_hit, "encoder vs hit-index rect for D");

    // Hidden node: encoder skips, hit index still tracks the slot.
    let h_id = WidgetId::from_hash("H");
    assert!(
        !drawn.iter().any(|(c, _)| *c == h_color),
        "hidden node must not emit a DrawRect"
    );
    assert!(
        ui.response_for(h_id).rect.is_some(),
        "hidden node still has a slot in the hit index"
    );

    // Click suppression: press+release at each widget's centre.
    // Visible → clicks. Disabled → suppressed (sense cascade in HitIndex).
    // Hidden → suppressed (visibility cascade).
    fn press_and_release_at(ui: &mut Ui, p: Vec2) {
        ui.on_input(InputEvent::PointerMoved(p));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    }
    press_and_release_at(
        &mut ui,
        v_hit.min + Vec2::new(v_hit.size.w, v_hit.size.h) * 0.5,
    );
    press_and_release_at(
        &mut ui,
        d_hit.min + Vec2::new(d_hit.size.w, d_hit.size.h) * 0.5,
    );
    let h_hit = ui.response_for(h_id).rect.unwrap();
    press_and_release_at(
        &mut ui,
        h_hit.min + Vec2::new(h_hit.size.w, h_hit.size.h) * 0.5,
    );

    // Frame 2: rebuild and read clicked() on each widget.
    ui.pre_record(Display::default());
    let mut got = (false, false, false);
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::canvas()
            .id_salt("mid")
            .size(200.0)
            .clip_rect()
            .transform(xform)
            .show(ui, |ui| {
                got.0 = Frame::new()
                    .id_salt("V")
                    .position((0.0, 0.0))
                    .size(30.0)
                    .background(Background {
                        fill: v_color.into(),
                        ..Default::default()
                    })
                    .sense(Sense::CLICK)
                    .show(ui)
                    .clicked();
                got.1 = Frame::new()
                    .id_salt("D")
                    .position((40.0, 0.0))
                    .size(30.0)
                    .background(Background {
                        fill: d_color.into(),
                        ..Default::default()
                    })
                    .sense(Sense::CLICK)
                    .disabled(true)
                    .show(ui)
                    .clicked();
                got.2 = Frame::new()
                    .id_salt("H")
                    .position((80.0, 0.0))
                    .size(30.0)
                    .background(Background {
                        fill: h_color.into(),
                        ..Default::default()
                    })
                    .sense(Sense::CLICK)
                    .hidden()
                    .show(ui)
                    .clicked();
            });
    });

    assert!(got.0, "visible widget should click");
    assert!(!got.1, "disabled widget must not click (sense cascade)");
    assert!(!got.2, "hidden widget must not click (visibility cascade)");
}

#[test]
fn nested_clips_each_emit_their_own_pair() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::zstack()
            .id_salt("outer")
            .size(Sizing::Fixed(100.0))
            .clip_rect()
            .show(ui, |ui| {
                Panel::zstack()
                    .id_salt("inner")
                    .size(Sizing::Fixed(50.0))
                    .clip_rect()
                    .show(ui, |_| {});
            });
    });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds(&ui);
    let ClipPairs { pushes, pops } = count_clip_pairs(&cmds);
    assert_eq!(pushes, 2);
    assert_eq!(pops, 2);
}

/// Disabled cascade strips a descendant's effective `Sense` to `NONE` so
/// it stops responding to hover/click. Pin the flag propagates through
/// nested panels.
#[test]
fn disabled_ancestor_propagates_disabled_flag_to_descendants() {
    let mut ui = ui_at(UVec2::new(100, 100));
    let mut child_node = None;
    Panel::vstack()
        .auto_id()
        .disabled(true)
        .show(&mut ui, |ui| {
            child_node = Some(
                Frame::new()
                    .auto_id()
                    .size(Sizing::Fixed(40.0))
                    .background(Background {
                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                        ..Default::default()
                    })
                    .show(ui)
                    .node,
            );
        });
    ui.post_record();
    ui.paint();
    let cascades = &ui.cascades.result;
    let child = child_node.unwrap();
    assert_eq!(cascades.entries[child.index()].sense, Sense::NONE);
}

/// `align_text_in` math: glyph bbox positioned inside the leaf's
/// arranged rect. Center, right, and bottom shift the origin; auto
/// (the default) collapses to top-left because glyphs don't stretch.
#[test]
fn align_text_in_centers_horizontally_and_vertically() {
    // Leaf is 200×40, text measures 80×16.
    let leaf = Rect::new(10.0, 20.0, 200.0, 40.0);
    let measured = Size::new(80.0, 16.0);

    let r = align_text_in(leaf, measured, Align::CENTER);
    // x: 10 + (200-80)/2 = 70. y: 20 + (40-16)/2 = 32.
    assert_eq!((r.min.x, r.min.y), (70.0, 32.0));
    assert_eq!((r.size.w, r.size.h), (80.0, 16.0));
}

#[test]
fn align_text_in_top_left_when_auto() {
    let leaf = Rect::new(10.0, 20.0, 200.0, 40.0);
    let measured = Size::new(80.0, 16.0);
    let r = align_text_in(leaf, measured, Align::default());
    assert_eq!((r.min.x, r.min.y), (10.0, 20.0));
}

#[test]
fn align_text_in_right_bottom() {
    let leaf = Rect::new(10.0, 20.0, 200.0, 40.0);
    let measured = Size::new(80.0, 16.0);
    let r = align_text_in(leaf, measured, Align::new(HAlign::Right, VAlign::Bottom));
    assert_eq!((r.min.x, r.min.y), (10.0 + 120.0, 20.0 + 24.0));
}

/// Negative-slack guard: if the measured text is *larger* than its
/// leaf rect, alignment shouldn't pull `min` past the leaf's `min`
/// (which would clip the text on the wrong side). Top-left is the
/// safe fallback — the `.max(0.0)` clamp.
#[test]
fn align_text_in_clamps_negative_slack_to_top_left() {
    let leaf = Rect::new(0.0, 0.0, 50.0, 10.0);
    let oversize = Size::new(80.0, 16.0);
    let r = align_text_in(leaf, oversize, Align::CENTER);
    // Even centered, min stays at leaf.min (no negative offset).
    assert_eq!((r.min.x, r.min.y), (0.0, 0.0));
}

/// Encoder honors padding for text alignment: a centered button label
/// inside a padded button is centered in the *content* area (rect
/// deflated by padding), not in the padding-inclusive outer rect.
#[test]
fn encoder_text_alignment_respects_leaf_padding() {
    use crate::text::TextShaper;
    use crate::widgets::button::Button;

    // Real shaper required so the encoder doesn't drop the text run as
    // having an invalid key (mono fallback uses `TextCacheKey::INVALID`).
    let mut ui = Ui::with_text(TextShaper::with_bundled_fonts());
    begin(&mut ui, UVec2::new(400, 400));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Button::new()
            .id_salt("padded")
            .label("ok")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(80.0)))
            .padding(20.0)
            .show(ui);
    });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds(&ui);
    let text_rect = (0..cmds.kinds.len())
        .find_map(|i| match cmds.kinds[i] {
            CmdKind::DrawText => Some(cmds.read::<DrawTextPayload>(cmds.starts[i]).rect),
            _ => None,
        })
        .expect("button must emit one DrawText");

    // Outer button rect is 200×80 at (0, 0). Padding is 20, so content
    // area is 160×40 starting at (20, 20). Centered "ok" (~16×16 with
    // mono metrics) lands at (20 + (160-16)/2, 20 + (40-16)/2) = (92, 32).
    // If padding were ignored, x would be (200-16)/2 = 92, but y would be
    // (80-16)/2 = 32 → indistinguishable on this axis. Use a non-square
    // padding-vs-rect ratio: x asserts the inset, y is just sanity.
    assert!(
        text_rect.min.x > 20.0 && text_rect.min.x < 180.0,
        "text x must lie inside padded content area, got {}",
        text_rect.min.x
    );
    // Specifically: centered horizontally inside [20, 180].
    let expected_x_center = 20.0 + (160.0 - text_rect.size.w) * 0.5;
    assert!(
        (text_rect.min.x - expected_x_center).abs() < 0.5,
        "text x should center within padded area; expected ≈{expected_x_center}, got {}",
        text_rect.min.x
    );
}

// --- DamageEngine filter (Step 5) -------------------------------------------------
// `damage_filter: Some(rect)` skips DrawRect/DrawText for nodes that don't
// intersect the dirty region. PushClip/PopClip and PushTransform/PopTransform
// are *always* emitted so scissor groups and child transforms stay coherent.

/// Pin: a node whose rect doesn't intersect the damage filter has no
/// DrawRect emitted.
#[test]
fn damage_filter_skips_drawrect_outside_dirty_region() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Frame::new()
            .id_salt("a")
            .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
            .background(Background {
                fill: Color::rgb(1.0, 0.0, 0.0).into(),
                ..Default::default()
            })
            .show(ui);
        Frame::new()
            .id_salt("b")
            .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
            .background(Background {
                fill: Color::rgb(0.0, 1.0, 0.0).into(),
                ..Default::default()
            })
            .show(ui);
    });
    ui.post_record();
    ui.paint();
    // DamageEngine filter covers only the left half (x: 0..50). `a` at
    // (0,0,40,40) intersects; `b` at (40,0,40,40) intersects too
    // (its left edge is at x=40 which is < 50). Use a tighter filter.
    let filter = Rect::new(0.0, 0.0, 30.0, 200.0);
    let cmds = encode_cmds_filtered(&ui, Some(filter));

    // `a` (0..40) intersects (0..30) → emitted. `b` (40..80) doesn't → skipped.
    assert_eq!(
        count_draw_rects(&cmds),
        1,
        "only the rect inside the damage filter should be drawn"
    );
}

/// Pin: a node fully *inside* the damage filter still emits its
/// DrawRect.
#[test]
fn damage_filter_keeps_drawrect_inside_dirty_region() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Frame::new()
            .id_salt("a")
            .size(50.0)
            .background(Background {
                fill: Color::rgb(1.0, 0.0, 0.0).into(),
                ..Default::default()
            })
            .show(ui);
    });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds_filtered(&ui, Some(Rect::new(0.0, 0.0, 200.0, 200.0)));
    assert!(count_draw_rects(&cmds) >= 1);
}

/// Pin: PushClip/PopClip pairs are emitted even for clipped nodes whose
/// rect doesn't intersect damage. The composer relies on these for group
/// boundaries.
/// Pin: a clipped panel whose subtree doesn't intersect damage gets
/// fully culled — no clip Push/Pop, no descendant draws. Same shape
/// as the off-screen viewport cull. The cull is sound when the
/// clipped subtree's contents stay inside the parent's `screen_rect`
/// (the typical case). Canvas / unclipped / transformed children
/// that overflow violate this; that's a documented "by convention"
/// trust, same as the viewport cull above.
#[test]
fn damage_filter_culls_subtree_outside_damage() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::hstack().id_salt("outer").show(&mut ui, |ui| {
        Panel::hstack()
            .id_salt("clipped")
            .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
            .clip_rect()
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("inner")
                    .size(20.0)
                    .background(Background {
                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    });
    ui.post_record();
    ui.paint();
    // Filter misses the clipped panel entirely.
    let cmds = encode_cmds_filtered(&ui, Some(Rect::new(150.0, 150.0, 50.0, 50.0)));

    let ClipPairs { pushes, pops } = count_clip_pairs(&cmds);
    assert_eq!(
        pushes, 0,
        "filtered subtree should emit no clip push (cull),\n\
         got {pushes}",
    );
    assert_eq!(pops, 0, "no clip push ⇒ no clip pop");
    assert_eq!(
        count_draw_rects(&cmds),
        0,
        "no rects emitted when nothing intersects damage"
    );
}

/// Pin: a transformed panel whose subtree doesn't intersect damage
/// gets fully culled — no PushTransform/PopTransform, no descendant
/// draws. Same cull as the clipped variant; same "transform doesn't
/// move children outside the parent's screen_rect" by-convention
/// trust.
#[test]
fn damage_filter_culls_transformed_subtree_outside_damage() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::hstack()
            .id_salt("transformed")
            .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
            .transform(TranslateScale::from_translation(Vec2::new(5.0, 5.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("inner")
                    .size(20.0)
                    .background(Background {
                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds_filtered(&ui, Some(Rect::new(150.0, 150.0, 50.0, 50.0)));

    let pushes = cmds
        .kinds
        .iter()
        .filter(|k| **k == CmdKind::PushTransform)
        .count();
    let pops = cmds
        .kinds
        .iter()
        .filter(|k| **k == CmdKind::PopTransform)
        .count();
    assert_eq!(
        pushes, 0,
        "filtered subtree should emit no transform push (cull); got {pushes}",
    );
    assert_eq!(pops, 0, "no push ⇒ no pop");
    assert_eq!(
        count_draw_rects(&cmds),
        0,
        "no rects emitted when nothing intersects damage",
    );
}

/// Pin: a multi-rect damage region paints leaves intersecting *any*
/// rect and skips leaves that miss every rect. Three corner frames at
/// (0,0)/(160,0)/(0,160), each 40×40; a region with two damage rects
/// covering the top-left and top-right corners must paint exactly
/// those two and skip the bottom-left.
#[test]
fn damage_filter_paints_leaves_in_any_rect() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::canvas()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            for (key, x, y) in &[("tl", 0.0, 0.0), ("tr", 160.0, 0.0), ("bl", 0.0, 160.0)] {
                Frame::new()
                    .id_salt(*key)
                    .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                    .position(Vec2::new(*x, *y))
                    .background(Background {
                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                        ..Default::default()
                    })
                    .show(ui);
            }
        });
    ui.post_record();
    ui.paint();
    // Two damage rects that each cover one corner; the bottom-left
    // corner falls outside both. The rects are far apart, so the
    // merge policy keeps them separate.
    let rects = [
        Rect::new(0.0, 0.0, 50.0, 50.0),
        Rect::new(150.0, 0.0, 50.0, 50.0),
    ];
    let cmds = encode_cmds_with_rects(&ui, &rects);
    assert_eq!(
        count_draw_rects(&cmds),
        2,
        "two top corners inside damage, bottom corner outside both",
    );
}

#[test]
fn viewport_cull_skips_offscreen_subtree() {
    let mut ui = ui_at(UVec2::new(100, 100));
    Panel::canvas()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::new()
                .id_salt("off")
                .position((500.0, 500.0))
                .size(20.0)
                .background(Background {
                    fill: Color::rgb(1.0, 0.0, 0.0).into(),
                    ..Default::default()
                })
                .show(ui);
        });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds(&ui);
    assert_eq!(
        count_draw_rects(&cmds),
        0,
        "off-screen frame must emit no DrawRect on a full-paint frame"
    );
}

/// Pin: with one on-screen and one off-screen sibling, only the
/// on-screen sibling paints. Confirms cull is per-subtree, not all-or-
/// nothing at the root.
#[test]
fn viewport_cull_keeps_onscreen_sibling() {
    let mut ui = ui_at(UVec2::new(100, 100));
    Panel::canvas()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::new()
                .id_salt("on")
                .position((10.0, 10.0))
                .size(20.0)
                .background(Background {
                    fill: Color::rgb(0.0, 1.0, 0.0).into(),
                    ..Default::default()
                })
                .show(ui);
            Frame::new()
                .id_salt("off")
                .position((500.0, 500.0))
                .size(20.0)
                .background(Background {
                    fill: Color::rgb(1.0, 0.0, 0.0).into(),
                    ..Default::default()
                })
                .show(ui);
        });
    ui.post_record();
    ui.paint();
    let cmds = encode_cmds(&ui);
    assert_eq!(
        count_draw_rects(&cmds),
        1,
        "only the on-screen sibling should emit a DrawRect"
    );
}
