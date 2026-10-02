//! The commands one recorded frame lowers to.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::brush::gradient::Spread;
use crate::primitives::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::color::rgba_f16::RgbaF16;
use crate::primitives::fill_axis::FillAxis;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::RgbaF32, rect::Rect, size::Size, stroke::Stroke};
use crate::renderer::frontend::encoder::GradientResolver;
use crate::renderer::frontend::encoder::tests::support::{
    as_rect, as_shadow, count_draw_rects, quad_rect,
};
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::scene::layer::Layer;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::scene::record_store::recorded_gradients::GradientId;
use crate::scene::shapes::paint::shape_brush::ShapeBrush;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::{UVec2, Vec2};

#[test]
fn gradient_resolution_runs_once_per_id_and_restarts_each_encode() {
    let gradient = RecordedGradient {
        axis: FillAxis::from_lanes(1.0, 0.0, 0.0, 1.0),
        kind: FillKind::linear(Spread::Pad),
        ramp: ColorRamp::two_stop(RgbaF32::BLACK, RgbaF32::WHITE),
    };
    let gradients = [gradient];
    let atlas = SharedGradientAtlas::default();
    let mut resolver = GradientResolver {
        atlas: atlas.clone(),
        resolved: Vec::new(),
    };
    let brush = ShapeBrush::Gradient {
        id: GradientId(0),
        hash: 0,
    };

    let mut pass = resolver.begin(&gradients);
    let first = pass.source(brush);
    let registered = atlas.registrations();
    let repeated = pass.source(brush);
    assert_eq!(atlas.registrations(), registered);
    match (first, repeated) {
        (BrushSource::Gradient(first), BrushSource::Gradient(repeated)) => {
            assert_eq!(first.axis, repeated.axis);
            assert_eq!(first.kind, repeated.kind);
            assert_eq!(first.lut_row, repeated.lut_row);
        }
        _ => panic!("gradient brush resolved to a solid source"),
    }

    let mut pass = resolver.begin(&gradients);
    assert!(pass.resolved[0].is_none());
    let _ = pass.source(brush);
    assert_eq!(atlas.registrations(), registered + 1);
}

/// Baseline encoder counts: empty tree emits no draws; a Frame with a
/// fill emits one rect quad; an invisible Frame (no fill / border /
/// shape) emits none — `ShapeRecord::is_noop` filters at `add_shape` time
/// so
/// the encoder sees no rectangle record in the tree. Degenerate Backgrounds
/// (transparent + no border) and clip-only Surfaces (`Surface::clip_rect`)
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
                    Block::new()
                        .id(WidgetId::from_hash("a"))
                        .size(50.0)
                        .background(Background::fill(RgbaF32::srgb(1.0, 0.0, 0.0)))
                        .show(ui);
                }
                Scene::InvisibleFrame => {
                    Block::new()
                        .id(WidgetId::from_hash("invisible"))
                        .size(50.0)
                        .show(ui);
                }
                Scene::FrameWithDegenerateBackground => {
                    Block::new()
                        .id(WidgetId::from_hash("degenerate"))
                        .size(50.0)
                        .background(Background {
                            fill: RgbaF32::TRANSPARENT.into(),
                            border: Stroke::ZERO,
                            ..Default::default()
                        })
                        .show(ui);
                }
                Scene::FrameWithClipRectSurface => {
                    Block::new()
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
    use crate::primitives::lut_row::LutRow;
    use crate::shape::Shape;

    let tint = RgbaF32::new(0.5, 0.25, 1.0, 0.5);
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            ui.add_shape(
                Shape::owner_rect()
                    .corners(4.0)
                    .fill(RgbaF32::srgb(1.0, 0.0, 0.0)),
            );
            ui.add_shape(
                Shape::owner_windowed_rect()
                    .corners(6.0)
                    .fill(RgbaF32::srgb(0.0, 1.0, 0.0)),
            );
            ui.add_shape(Shape::line(
                Vec2::new(0.0, 0.0),
                Vec2::new(20.0, 0.0),
                Stroke::new(RgbaF32::srgb(1.0, 0.0, 0.0), 2.0),
            ));
            ui.add_shape(
                Shape::line(
                    Vec2::new(0.0, 5.0),
                    Vec2::new(20.0, 5.0),
                    Stroke::new(tint, 2.0),
                )
                .ramp(ColorRamp::two_stop(RgbaF32::BLACK, RgbaF32::WHITE)),
            );
            // Degenerate variants: filtered before reaching the buffer.
            ui.add_shape(
                Shape::line(
                    Vec2::new(0.0, 0.0),
                    Vec2::new(10.0, 10.0),
                    Stroke::new(tint, 2.0),
                )
                .ramp(ColorRamp::two_stop(
                    RgbaF32::TRANSPARENT,
                    RgbaF32::TRANSPARENT,
                )),
            );
            ui.add_shape(Shape::line(
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 10.0),
                Stroke::new(RgbaF32::srgb(1.0, 0.0, 0.0), 0.0),
            ));
            ui.add_shape(Shape::line(
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 10.0),
                Stroke::new(RgbaF32::TRANSPARENT, 2.0),
            ));
            Block::new()
                .id(WidgetId::from_hash("host"))
                .size(50.0)
                .show(ui);
        });
    });
    let cmds = h.encode_paint();
    let rect_kinds: Vec<_> = cmds
        .calls
        .iter()
        .filter_map(|command| as_rect(command).map(|p| p.fill.kind))
        .collect();
    assert_eq!(
        rect_kinds,
        [FillKind::SOLID, FillKind::SOLID.with_window()],
        "the rounded rect is plain solid, the windowed one window-tagged",
    );
    // A Line rides the GPU curve pipeline (degenerate cubic), so it
    // emits a DrawCurve — not a DrawPolyline — and never touches the
    // polyline point payloads. The solid line carries its colour; the
    // ramp line carries the ramp kind, a real atlas row, and the stroke
    // colour as the multiplier on the sample.
    assert_eq!(cmds.kinds(), ["Quad", "Quad", "Curve", "Curve"]);
    let [solid, ramp] = [2, 3].map(|i| cmds.calls[i].as_curve().unwrap().fill);
    assert_eq!(
        (solid.kind, solid.color),
        (FillKind::SOLID, RgbaF32::srgb(1.0, 0.0, 0.0).into()),
    );
    assert_eq!((ramp.kind, ramp.color), (FillKind::RAMP, tint.into()));
    assert_ne!(
        ramp.lut_row,
        LutRow::FALLBACK,
        "the ramp resolved to a baked row"
    );
    assert_eq!(
        h.ui.forest().record_store.polyline_points.len(),
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
                    color: RgbaF32::srgba(0.0, 0.0, 0.0, 0.5),
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
                    color: RgbaF32::srgba(0.0, 0.0, 0.0, 0.5),
                    offset: Vec2::new(2.0, 4.0),
                    blur: 8.0,
                    spread: -2.0,
                    inset: true,
                })
                .at(Rect::new(10.0, 20.0, 30.0, 40.0))
                .corners(4.0),
            );
            Block::new()
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

    assert_eq!(drop.fill.kind, FillKind::SHADOW_DROP);
    assert_eq!(drop_rect.size, Size::new(78.0, 88.0));
    assert_eq!(drop_rect.min - inset_rect.min, Vec2::new(-22.0, -20.0));
    assert_eq!(drop.fill_axis.lanes(), [0.0, 0.0, 8.0, -1.0]);
    assert_eq!(
        drop.fill.color,
        RgbaF16::from(RgbaF32::srgba(0.0, 0.0, 0.0, 0.5))
    );
    // A shadow's whole edge is its blur — the merged payload must carry
    // no stroke, or the shared quad path would paint one.
    assert_eq!(drop.stroke.color, RgbaF16::TRANSPARENT);
    assert_eq!(drop.stroke.width, 0.0);

    assert_eq!(inset.fill.kind, FillKind::SHADOW_INSET);
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
    let key = h.ui.layout(Layer::Main).text_shapes[0].buffer_key();
    h.ui.shaper().drop_cosmic_buffers();
    assert!(
        !h.ui.shaper().has_cosmic_buffer(key),
        "fixture must evict the retained layout's key",
    );

    let cmds = h.encode_paint();
    assert_eq!(cmds.kinds(), ["Text"]);
    let payload = cmds.calls[0].as_text().unwrap();
    let scene = h.ui.frame_scene();
    let interned_text = scene.forest.record_store.interned_text();
    assert_eq!(interned_text.resolve(payload.text.span), "hi");
    assert!(
        !h.ui.shaper().has_cosmic_buffer(key),
        "frontend encoding must not reconstruct an evicted text buffer",
    );

    h.ui.shaper().drop_cosmic_buffers();
    let measure_calls = h.ui.shaper().measure_calls();
    h.ui.request_repaint();
    h.frame(body);
    let replayed_key = h.ui.layout(Layer::Main).text_shapes[0].buffer_key();
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
    assert_eq!(replayed.kinds(), ["Text"], "replayed text must still emit");
    let payload = replayed.calls[0].as_text().unwrap();
    let scene = h.ui.frame_scene();
    let interned_text = scene.forest.record_store.interned_text();
    assert_eq!(interned_text.resolve(payload.text.span), "hi");
    assert!(
        !h.ui.shaper().has_cosmic_buffer(replayed_key),
        "frontend replay must leave reconstruction to an encoded-cache miss",
    );
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
    assert_eq!(
        cmds.kinds(),
        ["Quad", "Text"],
        "the button's chrome, then its label"
    );
    let text_rect = cmds.calls[1].as_text().unwrap().rect;

    // "ok" is 19 px in the bundled face and one 20 px line, centred on
    // both axes inside the 20 px padding: 160 × 40 of room.
    assert_eq!(
        text_rect,
        Rect::new(
            20.0 + (160.0 - 19.0) * 0.5,
            20.0 + (40.0 - 20.0) * 0.5,
            19.0,
            20.0
        ),
    );
}
