use super::super::cmd_buffer::{
    CmdKind, DrawRectPayload, DrawTextPayload, PushClipPayload, RenderCmdBuffer,
};
use super::align_text_in;
use crate::Ui;
use crate::common::frame_arena::FrameArenaHandle;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::input::InputEvent;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::layout::types::{align::Align, align::HAlign, align::VAlign, sizing::Sizing};
use crate::primitives::background::Background;
use crate::primitives::shadow::Shadow;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{
    color::Color, rect::Rect, size::Size, stroke::Stroke, transform::TranslateScale,
};
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
        .filter(|k| matches!(k, CmdKind::DrawRect))
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
        let mut ui = Ui::for_test();
        ui.run_at_acked(UVec2::new(200, 200), |ui| {
            Panel::hstack().auto_id().show(ui, |ui| match scene {
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
        });
        let cmds = ui.encode_cmds();
        assert_eq!(count_draw_rects(&cmds), *expected, "case: {label}");
    }
}

/// Pin: the encoder iterates ALL shape variants in the background phase,
/// not just `Text`. Custom widgets pushing `Shape::RoundedRect` /
/// `Shape::Line` via `ui.add_shape` should still emit draw cmds; degenerate
/// `Line` variants are filtered at `add_shape` time.
#[test]
fn manually_pushed_shapes_emit_expected_cmds() {
    use crate::primitives::corners::Corners;
    use crate::shape::{LineCap, LineJoin, Shape};

    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            ui.add_shape(Shape::RoundedRect {
                local_rect: None,
                radius: Corners::all(4.0),
                fill: Color::rgb(1.0, 0.0, 0.0).into(),
                stroke: Stroke::ZERO,
            });
            ui.add_shape(Shape::Line {
                a: Vec2::new(0.0, 0.0),
                b: Vec2::new(20.0, 0.0),
                width: 2.0,
                brush: Color::rgb(1.0, 0.0, 0.0).into(),
                cap: LineCap::Butt,
                join: LineJoin::Miter,
            });
            // Degenerate variants: filtered before reaching the buffer.
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
    });
    let cmds = ui.encode_cmds();
    let draws = cmds
        .kinds
        .iter()
        .filter(|k| matches!(k, CmdKind::DrawRect))
        .count();
    assert!(draws >= 1, "RoundedRect must emit a DrawRect, got {draws}");
    let polylines = cmds
        .kinds
        .iter()
        .filter(|k| matches!(k, CmdKind::DrawPolyline))
        .count();
    assert_eq!(polylines, 1, "expected exactly one DrawPolyline cmd");
    // Points live on the Rc-shared `Ui.frame_arena`; the 2-point line
    // + noop-filtered duplicates leave exactly two entries.
    assert_eq!(
        ui.frame_arena.borrow().polyline_points.len(),
        2,
        "one 2-point line populates the points arena"
    );
}

/// `Shape::Shadow` lowers to a single `DrawShadow` cmd. The payload's
/// `fill_kind` is `FillKind::SHADOW_DROP` (4) and the paint bbox is
/// inflated by `|offset| + 3σ + spread` per side from the source
/// rect. `fill_axis` carries `(offset.x, offset.y, σ, _)`.
#[test]
fn shadow_lowers_to_drawshadow_with_inflated_bbox() {
    use crate::Shadow;
    use crate::primitives::corners::Corners;
    use crate::renderer::frontend::cmd_buffer::DrawShadowPayload;
    use crate::renderer::quad::FillKind;
    use crate::shape::Shape;

    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            ui.add_shape(Shape::Shadow {
                local_rect: Some(Rect::new(10.0, 20.0, 30.0, 40.0)),
                radius: Corners::all(4.0),
                shadow: Shadow {
                    color: Color::rgba(0.0, 0.0, 0.0, 0.5),
                    offset: Vec2::new(2.0, 4.0),
                    blur: 8.0,
                    spread: 1.0,
                    inset: false,
                },
            });
            Frame::new().id_salt("host").size(50.0).show(ui);
        });
    });
    let cmds = ui.encode_cmds();
    let shadow_payloads: Vec<DrawShadowPayload> = cmds
        .kinds
        .iter()
        .zip(cmds.starts.iter())
        .filter(|(k, _)| matches!(k, CmdKind::DrawShadow))
        .map(|(_, s)| cmds.read::<DrawShadowPayload>(*s))
        .collect();
    assert_eq!(shadow_payloads.len(), 1, "exactly one shadow cmd");
    let p = shadow_payloads[0];
    assert_eq!(p.fill_kind, FillKind::SHADOW_DROP);
    // Inflation: dx = |2| + 3*8 + 1 = 27, dy = |4| + 3*8 + 1 = 29.
    // Source is (10,20)..(40,60); paint bbox = (-17,-9)..(67,89).
    // Owner rect min comes from Panel layout, which we don't know
    // exactly — just assert the size and that fill_axis carries the
    // raw offset/σ.
    assert!(
        (p.rect.size.w - 84.0).abs() < 0.5,
        "paint bbox width = source.w + 2*dx = 30 + 54 = 84, got {}",
        p.rect.size.w
    );
    assert!(
        (p.rect.size.h - 98.0).abs() < 0.5,
        "paint bbox height = source.h + 2*dy = 40 + 58 = 98, got {}",
        p.rect.size.h
    );
    let [dx, dy, t0, _t1] = p.fill_axis.lanes();
    assert_eq!(dx, 2.0);
    assert_eq!(dy, 4.0);
    assert_eq!(t0, 8.0);
    assert_eq!(
        p.color,
        crate::primitives::color::ColorF16::from(Color::rgba(0.0, 0.0, 0.0, 0.5))
    );
}

#[test]
fn text_shape_emits_draw_text() {
    use crate::Text;
    let mut ui = Ui::for_test_at_text(UVec2::new(200, 200));
    ui.run_at_acked(UVec2::new(200, 200), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Text::new("hi").auto_id().show(ui);
        });
    });
    let cmds = ui.encode_cmds();
    assert!(
        cmds.kinds.contains(&CmdKind::DrawText),
        "Text widget must emit a DrawText command"
    );
}

/// Pin: a clip-only Surface (no painted background) still emits a
/// PushClip/PopClip pair so children get clipped, while contributing zero
/// `DrawRect`s of its own.
#[test]
fn clip_only_surface_emits_clip_but_no_draw() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id_salt("clip_only")
                .size(50.0)
                .clip_rect()
                .show(ui, |_| {});
        });
    });
    let cmds = ui.encode_cmds();
    let ClipPairs { pushes, pops } = count_clip_pairs(&cmds);
    assert_eq!(pushes, 1);
    assert_eq!(pops, 1);
    assert_eq!(count_draw_rects(&cmds), 0);
}

#[test]
fn clip_emits_balanced_push_pop() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
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
    });
    let cmds = ui.encode_cmds();

    let ClipPairs { pushes, pops } = count_clip_pairs(&cmds);
    assert_eq!(pushes, 1);
    assert_eq!(pops, 1);

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
        .filter_map(|(i, k)| matches!(k, CmdKind::DrawRect).then_some(i))
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
/// the mask matches the panel rect and radius verbatim — stroke is chrome
/// only and doesn't deflate the clip.
#[test]
fn clip_rounded_emits_push_clip_rounded_when_background_has_radius() {
    use crate::primitives::corners::Corners;
    let mut ui = Ui::for_test();
    let mut panel_node = None;
    ui.run_at_acked(UVec2::new(200, 200), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            panel_node = Some(
                Panel::zstack()
                    .id_salt("rounded")
                    .size(80.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.2, 0.2).into(),
                        stroke: Stroke::solid(Color::rgb(1.0, 1.0, 1.0), 2.0),
                        radius: Corners::all(8.0),
                        shadow: Shadow::NONE,
                    })
                    .clip_rounded()
                    .show(ui, |ui| {
                        Frame::new().id_salt("c").size(40.0).show(ui);
                    })
                    .node(ui),
            );
        });
    });
    let cmds = ui.encode_cmds();

    let rounded_idx = cmds
        .kinds
        .iter()
        .enumerate()
        .find_map(|(idx, k)| {
            if *k != CmdKind::PushClip {
                return None;
            }
            let payload: PushClipPayload = cmds.read(cmds.starts[idx]);
            (!payload.radius.approx_zero()).then_some(idx)
        })
        .expect("rounded clip with rounded background emits PushClip with non-zero radius");
    let rounded_count = cmds
        .kinds
        .iter()
        .enumerate()
        .filter(|(idx, k)| {
            **k == CmdKind::PushClip && {
                let p: PushClipPayload = cmds.read(cmds.starts[*idx]);
                !p.radius.approx_zero()
            }
        })
        .count();
    assert_eq!(rounded_count, 1);

    let panel_rect = ui.layout[Layer::Main].rect[panel_node.unwrap().index()];
    let start = cmds.starts[rounded_idx];
    let payload: PushClipPayload = cmds.read(start);
    assert_eq!(payload.rect, panel_rect);
    assert_eq!(payload.radius, Corners::all(8.0));
}

#[test]
fn clip_rounded_falls_back_to_scissor_without_background() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id_salt("rounded_no_bg")
                .size(80.0)
                .clip_rounded()
                .show(ui, |ui| {
                    Frame::new().id_salt("c").size(40.0).show(ui);
                });
        });
    });
    let cmds = ui.encode_cmds();
    let push_clips: Vec<PushClipPayload> = cmds
        .kinds
        .iter()
        .enumerate()
        .filter(|(_, k)| **k == CmdKind::PushClip)
        .map(|(idx, _)| cmds.read::<PushClipPayload>(cmds.starts[idx]))
        .collect();
    assert_eq!(push_clips.len(), 1);
    assert!(
        push_clips[0].radius.approx_zero(),
        "no background → no radius → falls back to plain scissor",
    );
}

/// Walk an encoder command stream and return the effective screen-space rect
/// for each `DrawRect`, keyed by its fill colour.
fn screen_rects_by_fill(cmds: &RenderCmdBuffer) -> Vec<(crate::primitives::color::ColorF16, Rect)> {
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
            CmdKind::PushClip => {
                let p: PushClipPayload = cmds.read(start);
                let screen = t.apply_rect(p.rect);
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
            CmdKind::DrawShadow
            | CmdKind::DrawText
            | CmdKind::DrawMesh
            | CmdKind::DrawPolyline
            | CmdKind::DrawImage => {}
        }
    }
    assert!(t_stack.is_empty(), "transform stack unbalanced");
    assert!(clip_stack.is_empty(), "clip stack unbalanced");
    out
}

#[test]
fn cascade_matches_hit_index_for_visible_disabled_and_hidden() {
    // Visible and disabled get the same effective screen rect; hidden is
    // skipped by encoder but tracked by hit index. Clicks land on visible
    // and are suppressed for both disabled (sense cascade) and hidden
    // (visibility cascade).
    let v_color = Color::rgb(1.0, 0.0, 0.0);
    let d_color = Color::rgb(0.0, 1.0, 0.0);
    let h_color = Color::rgb(0.0, 0.0, 1.0);
    let xform = TranslateScale::new(Vec2::new(5.0, 7.0), 2.0);

    let surface = UVec2::new(400, 400);
    let build = |ui: &mut Ui, capture: &mut (bool, bool, bool)| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id_salt("mid")
                .size(200.0)
                .clip_rect()
                .transform(xform)
                .show(ui, |ui| {
                    capture.0 |= Frame::new()
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
                    capture.1 |= Frame::new()
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
                    capture.2 |= Frame::new()
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
    };

    let mut ui = Ui::for_test();
    let mut sink = (false, false, false);
    ui.run_at_acked(surface, |ui| build(ui, &mut sink));

    let cmds = ui.encode_cmds();
    let drawn = screen_rects_by_fill(&cmds);

    // Encoder stores fills as `ColorF16` now; encode the expected
    // colours the same way for bit-exact comparison.
    use crate::primitives::color::ColorF16;
    let v_color_f16: ColorF16 = v_color.into();
    let d_color_f16: ColorF16 = d_color.into();
    let h_color_f16: ColorF16 = h_color.into();

    let v_id = WidgetId::from_hash("V");
    let v_screen = drawn
        .iter()
        .find(|(c, _)| *c == v_color_f16)
        .map(|(_, r)| *r)
        .expect("visible node should emit a DrawRect");
    let v_hit = ui.response_for(v_id).rect.expect("visible has hit rect");
    assert_eq!(v_screen, v_hit, "encoder vs hit-index rect for V");

    let d_id = WidgetId::from_hash("D");
    let d_screen = drawn
        .iter()
        .find(|(c, _)| *c == d_color_f16)
        .map(|(_, r)| *r)
        .expect("disabled node should still paint");
    let d_hit = ui.response_for(d_id).rect.expect("disabled has rect");
    assert_eq!(d_screen, d_hit, "encoder vs hit-index rect for D");

    let h_id = WidgetId::from_hash("H");
    assert!(
        !drawn.iter().any(|(c, _)| *c == h_color_f16),
        "hidden node must not emit a DrawRect"
    );
    assert!(ui.response_for(h_id).rect.is_some());

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

    let mut got = (false, false, false);
    ui.run_at_acked(surface, |ui| build(ui, &mut got));
    assert!(got.0, "visible widget should click");
    assert!(!got.1, "disabled widget must not click (sense cascade)");
    assert!(!got.2, "hidden widget must not click (visibility cascade)");
}

#[test]
fn nested_clips_each_emit_their_own_pair() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
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
    });
    let cmds = ui.encode_cmds();
    let ClipPairs { pushes, pops } = count_clip_pairs(&cmds);
    assert_eq!(pushes, 2);
    assert_eq!(pops, 2);
}

#[test]
fn disabled_ancestor_propagates_disabled_flag_to_descendants() {
    let mut ui = Ui::for_test();
    let mut child_node = None;
    ui.run_at_acked(UVec2::new(100, 100), |ui| {
        Panel::vstack().auto_id().disabled(true).show(ui, |ui| {
            child_node = Some(
                Frame::new()
                    .auto_id()
                    .size(Sizing::Fixed(40.0))
                    .background(Background {
                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                        ..Default::default()
                    })
                    .show(ui)
                    .node(ui),
            );
        });
    });
    let cascades = &ui.layout.cascades;
    let child = child_node.unwrap();
    assert_eq!(cascades.entries[child.index()].sense, Sense::NONE);
}

/// `align_text_in` math: glyph bbox positioned inside the leaf's arranged
/// rect. Auto/center/right-bottom shift the origin; oversize content
/// clamps to top-left so it doesn't clip on the wrong side.
#[test]
fn align_text_in_cases() {
    let leaf = Rect::new(10.0, 20.0, 200.0, 40.0);
    let measured = Size::new(80.0, 16.0);

    let r = align_text_in(leaf, measured, Align::CENTER);
    assert_eq!((r.min.x, r.min.y), (70.0, 32.0));
    assert_eq!((r.size.w, r.size.h), (80.0, 16.0));

    let r = align_text_in(leaf, measured, Align::default());
    assert_eq!((r.min.x, r.min.y), (10.0, 20.0));

    let r = align_text_in(leaf, measured, Align::new(HAlign::Right, VAlign::Bottom));
    assert_eq!((r.min.x, r.min.y), (10.0 + 120.0, 20.0 + 24.0));

    // Negative-slack guard: oversize text clamps to top-left.
    let small = Rect::new(0.0, 0.0, 50.0, 10.0);
    let oversize = Size::new(80.0, 16.0);
    let r = align_text_in(small, oversize, Align::CENTER);
    assert_eq!((r.min.x, r.min.y), (0.0, 0.0));
}

#[test]
fn encoder_text_alignment_respects_leaf_padding() {
    use crate::text::TextShaper;
    use crate::widgets::button::Button;

    let mut ui = Ui::new(
        TextShaper::with_bundled_fonts(),
        FrameArenaHandle::default(),
        crate::primitives::image::ImageRegistry::default(),
    );
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new()
                .id_salt("padded")
                .label("ok")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(80.0)))
                .padding(20.0)
                .show(ui);
        });
    });
    let cmds = ui.encode_cmds();
    let text_rect = (0..cmds.kinds.len())
        .find_map(|i| match cmds.kinds[i] {
            CmdKind::DrawText => Some(cmds.read::<DrawTextPayload>(cmds.starts[i]).rect),
            _ => None,
        })
        .expect("button must emit one DrawText");

    assert!(
        text_rect.min.x > 20.0 && text_rect.min.x < 180.0,
        "text x must lie inside padded content area, got {}",
        text_rect.min.x
    );
    let expected_x_center = 20.0 + (160.0 - text_rect.size.w) * 0.5;
    assert!(
        (text_rect.min.x - expected_x_center).abs() < 0.5,
        "text x should center within padded area; expected ≈{expected_x_center}, got {}",
        text_rect.min.x
    );
}

// --- DamageEngine filter ---------------------------------------------

#[test]
fn damage_filter_partitions_drawrects_by_dirty_region() {
    let cases: &[(&str, Rect, usize)] = &[
        (
            "outside_filter_skipped",
            Rect::new(0.0, 0.0, 30.0, 200.0),
            1,
        ),
        ("inside_filter_kept", Rect::new(0.0, 0.0, 200.0, 200.0), 2),
    ];
    for (label, filter, expected) in cases {
        let mut ui = Ui::for_test();
        ui.run_at_acked(UVec2::new(200, 200), |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
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
        });
        let cmds = ui.encode_cmds_filtered(Some(*filter));
        assert_eq!(count_draw_rects(&cmds), *expected, "case: {label}");
    }
}

/// Cull subtree when filter misses it: clipped or transformed parent's
/// Push/Pop and descendant draws all suppressed. By-convention trust:
/// children stay inside the parent's screen_rect.
#[test]
fn damage_filter_culls_subtree_outside_damage() {
    enum Wrap {
        Clipped,
        Transformed,
    }
    let cases: &[(&str, Wrap, CmdKind, CmdKind)] = &[
        (
            "clipped",
            Wrap::Clipped,
            CmdKind::PushClip,
            CmdKind::PopClip,
        ),
        (
            "transformed",
            Wrap::Transformed,
            CmdKind::PushTransform,
            CmdKind::PopTransform,
        ),
    ];
    for (label, wrap, push_kind, pop_kind) in cases {
        let mut ui = Ui::for_test();
        ui.run_at_acked(UVec2::new(200, 200), |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                let inner = |ui: &mut Ui| {
                    Frame::new()
                        .id_salt("inner")
                        .size(20.0)
                        .background(Background {
                            fill: Color::rgb(1.0, 0.0, 0.0).into(),
                            ..Default::default()
                        })
                        .show(ui);
                };
                match wrap {
                    Wrap::Clipped => Panel::hstack()
                        .id_salt("clipped")
                        .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                        .clip_rect()
                        .show(ui, inner),
                    Wrap::Transformed => Panel::hstack()
                        .id_salt("transformed")
                        .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                        .transform(TranslateScale::from_translation(Vec2::new(5.0, 5.0)))
                        .show(ui, inner),
                };
            });
        });
        let cmds = ui.encode_cmds_filtered(Some(Rect::new(150.0, 150.0, 50.0, 50.0)));
        let pushes = cmds.kinds.iter().filter(|k| *k == push_kind).count();
        let pops = cmds.kinds.iter().filter(|k| *k == pop_kind).count();
        assert_eq!(pushes, 0, "case {label}: no push (cull)");
        assert_eq!(pops, 0, "case {label}: no pop");
        assert_eq!(count_draw_rects(&cmds), 0, "case {label}: no draws");
    }
}

#[test]
fn damage_filter_paints_leaves_in_any_rect() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
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
    });
    let rects = [
        Rect::new(0.0, 0.0, 50.0, 50.0),
        Rect::new(150.0, 0.0, 50.0, 50.0),
    ];
    let cmds = ui.encode_cmds_with_rects(&rects);
    assert_eq!(
        count_draw_rects(&cmds),
        2,
        "two top corners inside damage, bottom corner outside both",
    );
}

/// Viewport cull is per-subtree: an off-screen sibling drops while an
/// on-screen sibling paints.
#[test]
fn viewport_cull_skips_offscreen_subtree() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(100, 100), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
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
    });
    let cmds = ui.encode_cmds();
    assert_eq!(
        count_draw_rects(&cmds),
        1,
        "only the on-screen sibling should emit a DrawRect"
    );
}

/// Pin: image `fit` resolution. A 100×50 image painted into a 200×200
/// rect produces a different paint rect for each fit mode:
/// - `Fill` keeps the full 200×200 rect (image stretched).
/// - `Contain` scales by min(200/100, 200/50)=2 → 200×100, centered.
/// - `Cover` scales by max(200/100, 200/50)=4 → 400×200 conceptually,
///   but rendered at full 200×200 with UV-cropped to (0.5..1.0)
///   vertical band of the texture (`uv_size.y = 50/200 = 0.25`).
/// - `None` paints at intrinsic 100×50 centered.
#[test]
fn image_fit_modes_resolve_to_expected_rects_and_uv() {
    use super::resolve_fit;
    use crate::ImageFit;
    use glam::{UVec2, Vec2};

    let base = Rect::new(0.0, 0.0, 200.0, 200.0);
    let img = UVec2::new(100, 50);

    let r = resolve_fit(base, img, ImageFit::Fill);
    assert_eq!(r.rect, base);
    assert_eq!(r.uv_min, Vec2::ZERO);
    assert_eq!(r.uv_size, Vec2::ONE);

    let r = resolve_fit(base, img, ImageFit::Contain);
    assert_eq!(r.rect, Rect::new(0.0, 50.0, 200.0, 100.0));
    assert_eq!(r.uv_size, Vec2::ONE);

    let r = resolve_fit(base, img, ImageFit::Cover);
    assert_eq!(r.rect, base);
    // 200×200 paint rect over a 400×200 scaled image → keep 0.5 of the
    // width centered; full height. UVs sample the centered band.
    assert!((r.uv_size.x - 0.5).abs() < 1e-5);
    assert!((r.uv_size.y - 1.0).abs() < 1e-5);
    assert!((r.uv_min.x - 0.25).abs() < 1e-5);
    assert!((r.uv_min.y - 0.0).abs() < 1e-5);

    let r = resolve_fit(base, img, ImageFit::None);
    assert_eq!(r.rect, Rect::new(50.0, 75.0, 100.0, 50.0));
    assert_eq!(r.uv_size, Vec2::ONE);

    // Missing registry entry → falls through to base + full UV.
    let r = resolve_fit(base, UVec2::ZERO, ImageFit::Contain);
    assert_eq!(r.rect, base);
    assert_eq!(r.uv_size, Vec2::ONE);
}
