//! The commands one recorded frame lowers to.

use crate::Ui;
use crate::layout::types::{align::Align, align::HAlign, align::VAlign, sizing::Sizing};
use crate::primitives::background::Background;
use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::brush::gradient::{Interp, Spread};
use crate::primitives::color::ColorF16;
use crate::primitives::color::ColorU8;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, rect::Rect, size::Size, stroke::Stroke};
use crate::renderer::frontend::capture::PaintCall;
use crate::renderer::frontend::encoder::GradientResolver;
use crate::renderer::frontend::encoder::tests::support::{as_rect, count_draw_rects, quad_rect};
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::scene::record_store::recorded_gradients::GradientId;
use crate::scene::shapes::paint::ShapeBrush;
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

#[test]
fn gradient_resolution_runs_once_per_id_and_restarts_each_encode() {
    let gradient = RecordedGradient {
        axis: FillAxis::from_lanes(1.0, 0.0, 0.0, 1.0),
        kind: FillKind::linear(Spread::Pad),
        stops: GradientStops::new([
            Stop::new(0.0, ColorU8::BLACK),
            Stop::new(1.0, ColorU8::WHITE),
        ]),
        interp: Interp::Oklab,
    };
    let gradients = [gradient];
    let atlas = SharedGradientAtlas::default();
    let mut resolver = GradientResolver::default();
    let brush = ShapeBrush::Gradient(GradientId(0));

    resolver.begin(gradients.len());
    let first = resolver.source(&gradients, &atlas, brush);
    let registered = atlas.registrations();
    let repeated = resolver.source(&gradients, &atlas, brush);
    assert_eq!(atlas.registrations(), registered);
    match (first, repeated) {
        (BrushSource::Gradient(first), BrushSource::Gradient(repeated)) => {
            assert_eq!(first.axis, repeated.axis);
            assert_eq!(first.kind, repeated.kind);
            assert_eq!(first.row, repeated.row);
        }
        _ => panic!("gradient brush resolved to a solid source"),
    }

    resolver.begin(gradients.len());
    assert!(resolver.resolved[0].is_none());
    let _ = resolver.source(&gradients, &atlas, brush);
    assert_eq!(atlas.registrations(), registered + 1);
}

/// Baseline encoder counts: empty tree emits no draws; a Frame with a
/// fill emits one rect quad; an invisible Frame (no fill / stroke /
/// shape) emits none — `ShapeRecord::is_noop` filters at `add_shape` time
/// so
/// the encoder sees no rectangle record in the tree. Degenerate Backgrounds
/// (transparent + no stroke) and clip-only Surfaces (`Surface::clip_rect`)
/// also emit zero rect quads — the encoder's `bg.is_noop()` guard at
/// chrome-paint time filters them.
#[test]
fn baseline_draw_rect_count_cases() {
    #[derive(Debug)]
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
        let mut h = UiHarness::new(UVec2::new(200, 200));
        h.frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| match scene {
                Scene::Empty => {}
                Scene::FrameWithFill => {
                    Frame::new()
                        .id(WidgetId::from_hash("a"))
                        .size(50.0)
                        .background(Background {
                            fill: Color::rgb(1.0, 0.0, 0.0).into(),
                            ..Default::default()
                        })
                        .show(ui);
                }
                Scene::InvisibleFrame => {
                    Frame::new()
                        .id(WidgetId::from_hash("invisible"))
                        .size(50.0)
                        .show(ui);
                }
                Scene::FrameWithDegenerateBackground => {
                    Frame::new()
                        .id(WidgetId::from_hash("degenerate"))
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
                        .id(WidgetId::from_hash("clip_only"))
                        .size(50.0)
                        .clip_rect()
                        .show(ui);
                }
            });
        });
        let cmds = h.encode_paint();
        assert_eq!(count_draw_rects(&cmds), *expected, "case: {label}");
    }
}

/// Pin: the encoder iterates ALL shape variants in the background phase,
/// not just `Text`. Custom widgets pushing `Shape::rect` /
/// `Shape::line` via `ui.add_shape` should still emit draw cmds; degenerate
/// `Line` variants are filtered at `add_shape` time.
#[test]
fn manually_pushed_shapes_emit_expected_cmds() {
    use crate::shape::Shape;

    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            ui.add_shape(
                Shape::owner_rect()
                    .corners(4.0)
                    .fill(Color::rgb(1.0, 0.0, 0.0)),
            );
            ui.add_shape(
                Shape::owner_windowed_rect()
                    .corners(6.0)
                    .fill(Color::rgb(0.0, 1.0, 0.0)),
            );
            ui.add_shape(
                Shape::line(Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0), 2.0)
                    .brush(Color::rgb(1.0, 0.0, 0.0)),
            );
            // Degenerate variants: filtered before reaching the buffer.
            ui.add_shape(
                Shape::line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0), 0.0)
                    .brush(Color::rgb(1.0, 0.0, 0.0)),
            );
            ui.add_shape(
                Shape::line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0), 2.0)
                    .brush(Color::TRANSPARENT),
            );
            Frame::new()
                .id(WidgetId::from_hash("host"))
                .size(50.0)
                .show(ui);
        });
    });
    let cmds = h.encode_paint();
    let rect_kinds: Vec<_> = cmds
        .calls
        .iter()
        .filter_map(|command| as_rect(command).map(|p| p.fill_kind))
        .collect();
    assert!(
        rect_kinds.contains(&FillKind::SOLID),
        "rounded rect must emit a plain-solid quad, got kinds {rect_kinds:?}",
    );
    assert!(
        rect_kinds.contains(&FillKind::SOLID.with_window()),
        "windowed rect must emit a window-tagged quad, got kinds {rect_kinds:?}",
    );
    // A Line rides the GPU curve pipeline (degenerate cubic), so it
    // emits a DrawCurve — not a DrawPolyline — and never touches the
    // polyline point payloads.
    let curves = cmds
        .calls
        .iter()
        .filter(|command| matches!(command, PaintCall::Curve(_)))
        .count();
    assert_eq!(curves, 1, "expected exactly one DrawCurve cmd");
    assert_eq!(
        cmds.calls
            .iter()
            .filter(|command| matches!(command, PaintCall::Polyline(_)))
            .count(),
        0,
        "lines no longer lower to polylines"
    );
    assert_eq!(
        h.ui.forest()
            .record_store
            .payloads
            .borrow()
            .polyline_points
            .len(),
        0,
        "the point payloads stay untouched by lines"
    );
}

/// Drop shadows lower around their shifted source and no longer need
/// offset lanes in the shader payload. Inset shadows retain the source
/// bbox and offset/spread lanes because the shader moves the inner hole.
#[test]
fn shadows_lower_to_shifted_drop_and_source_bounded_inset() {
    use crate::Shadow;

    use crate::primitives::fill_kind::FillKind;
    use crate::shape::Shape;

    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            ui.add_shape(
                Shape::shadow(Shadow {
                    color: Color::rgba(0.0, 0.0, 0.0, 0.5),
                    offset: Vec2::new(2.0, 4.0),
                    blur: 8.0,
                    spread: -1.0,
                    inset: false,
                })
                .at(Rect::new(10.0, 20.0, 30.0, 40.0))
                .corners(4.0),
            );
            ui.add_shape(
                Shape::shadow(Shadow {
                    color: Color::rgba(0.0, 0.0, 0.0, 0.5),
                    offset: Vec2::new(2.0, 4.0),
                    blur: 8.0,
                    spread: -2.0,
                    inset: true,
                })
                .at(Rect::new(10.0, 20.0, 30.0, 40.0))
                .corners(4.0),
            );
            Frame::new()
                .id(WidgetId::from_hash("host"))
                .size(50.0)
                .show(ui);
        });
    });
    let cmds = h.encode_paint();
    let shadow_payloads: Vec<_> = cmds.calls.iter().filter_map(as_shadow).collect();
    assert_eq!(shadow_payloads.len(), 2, "drop and inset shadow cmds");
    let drop = shadow_payloads[0];
    let inset = shadow_payloads[1];
    let (drop_rect, inset_rect) = (quad_rect(drop), quad_rect(inset));

    assert_eq!(drop.fill_kind, FillKind::SHADOW_DROP);
    assert_eq!(drop_rect.size, Size::new(78.0, 88.0));
    assert_eq!(drop_rect.min - inset_rect.min, Vec2::new(-22.0, -20.0));
    assert_eq!(drop.fill_axis.lanes(), [0.0, 0.0, 8.0, -1.0]);
    assert_eq!(drop.fill, ColorF16::from(Color::rgba(0.0, 0.0, 0.0, 0.5)));
    // A shadow's whole edge is its blur — the merged payload must carry
    // no stroke, or the shared quad path would paint one.
    assert_eq!(drop.stroke.color, ColorF16::TRANSPARENT);
    assert_eq!(drop.stroke.width, 0.0);

    assert_eq!(inset.fill_kind, FillKind::SHADOW_INSET);
    assert_eq!(inset_rect.size, Size::new(30.0, 40.0));
    assert_eq!(inset.fill_axis.lanes(), [2.0, 4.0, 8.0, -2.0]);
}

#[test]
fn text_shape_carries_source_without_reconstructing_buffer() {
    use crate::Text;
    fn body(ui: &mut Ui) {
        Panel::hstack().auto_id().show(ui, |ui| {
            Text::new("hi").auto_id().show(ui);
        });
    }

    let mut h = UiHarness::with_text(UVec2::new(200, 200));
    h.frame(body);
    let key = h.ui.layout(Layer::Main).text_shapes[0].key;
    h.ui.shaper().drop_cosmic_buffers();
    assert!(
        !h.ui.shaper().has_cosmic_buffer(key),
        "fixture must evict the retained layout's key",
    );

    let cmds = h.encode_paint();
    let payload = cmds
        .calls
        .iter()
        .find_map(|command| match command {
            PaintCall::Text(payload) => Some(payload),
            _ => None,
        })
        .expect("Text widget must emit a DrawText command");
    let scene = h.ui.frame_scene();
    let interned_text = scene.payloads.interned_text();
    assert_eq!(payload.text.source.resolve(&interned_text), "hi");
    assert!(
        !h.ui.shaper().has_cosmic_buffer(key),
        "frontend encoding must not reconstruct an evicted text buffer",
    );
    drop(scene);

    h.ui.shaper().drop_cosmic_buffers();
    let measure_calls = h.ui.shaper().measure_calls();
    h.ui.request_repaint();
    h.frame(body);
    let replayed_key = h.ui.layout(Layer::Main).text_shapes[0].key;
    assert_eq!(replayed_key, key);
    assert_eq!(
        h.ui.shaper().measure_calls(),
        measure_calls,
        "unchanged full record must replay text layout without reshaping",
    );
    assert!(
        !h.ui.shaper().has_cosmic_buffer(replayed_key),
        "layout replay must be allowed to retain an evicted cache key",
    );
    let replayed = h.encode_paint();
    let payload = replayed
        .calls
        .iter()
        .find_map(|command| match command {
            PaintCall::Text(payload) => Some(payload),
            _ => None,
        })
        .expect("replayed text must still emit");
    let scene = h.ui.frame_scene();
    let interned_text = scene.payloads.interned_text();
    assert_eq!(payload.text.source.resolve(&interned_text), "hi");
    assert!(
        !h.ui.shaper().has_cosmic_buffer(replayed_key),
        "frontend replay must leave reconstruction to an encoded-cache miss",
    );
}

/// `Align::place_in` math: glyph bbox positioned inside the leaf's arranged
/// rect. Auto/center/right-bottom shift the origin; oversize content
/// clamps to top-left so it doesn't clip on the wrong side.
#[test]
fn place_in_cases() {
    let leaf = Rect::new(10.0, 20.0, 200.0, 40.0);
    let measured = Size::new(80.0, 16.0);

    let r = Align::CENTER.place_in(leaf, measured);
    assert_eq!((r.min.x, r.min.y), (70.0, 32.0));
    assert_eq!((r.size.w, r.size.h), (80.0, 16.0));

    let r = Align::default().place_in(leaf, measured);
    assert_eq!((r.min.x, r.min.y), (10.0, 20.0));

    let r = Align::new(HAlign::Right, VAlign::Bottom).place_in(leaf, measured);
    assert_eq!((r.min.x, r.min.y), (10.0 + 120.0, 20.0 + 24.0));

    // Negative-slack guard: oversize text clamps to top-left.
    let small = Rect::new(0.0, 0.0, 50.0, 10.0);
    let oversize = Size::new(80.0, 16.0);
    let r = Align::CENTER.place_in(small, oversize);
    assert_eq!((r.min.x, r.min.y), (0.0, 0.0));
}

#[test]
fn encoder_text_alignment_respects_leaf_padding() {
    use crate::widgets::button::Button;

    let mut h = UiHarness::with_text(UVec2::new(400, 400));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new()
                .id(WidgetId::from_hash("padded"))
                .label("ok")
                .size((Sizing::fixed(200.0), Sizing::fixed(80.0)))
                .padding(20.0)
                .show(ui);
        });
    });
    let cmds = h.encode_paint();
    let text_rect = cmds
        .calls
        .iter()
        .find_map(|command| match command {
            PaintCall::Text(payload) => Some(payload.rect),
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

/// The shadow half of the same split.
fn as_shadow(call: &PaintCall) -> Option<&DrawQuadPayload> {
    match call {
        PaintCall::Quad(p) if p.fill_kind.is_shadow() => Some(p),
        _ => None,
    }
}
