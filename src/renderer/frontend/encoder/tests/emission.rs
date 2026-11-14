//! The commands one recorded frame lowers to.

use crate::common::content_hash::ContentHash;
use crate::host::harness::Harness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::gradient::Spread;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::frontend::encoder::gradient_resolver::GradientResolver;
use crate::renderer::frontend::encoder::tests::support::{
    as_rect, as_shadow, count_draw_rects, quad_rect,
};
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_stroke_payload::StrokePath;
use crate::renderer::row_table::RowTable;
use crate::scene::layer::Layer;
use crate::shape::geometry::curve_basis::CurveBasis;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::shape::record_store::recorded_gradient::RecordedGradient;
use crate::shape::record_store::recorded_gradients::GradientId;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::{UVec2, Vec2};

#[test]
fn gradient_resolution_runs_once_per_id_and_restarts_each_encode() {
    let gradient = RecordedGradient {
        axis: FillAxis::from_lanes([1.0, 0.0, 0.0, 1.0]),
        kind: FillKind::linear(Spread::Pad),
        ramp: ColorRamp::from([RgbaF32::BLACK, RgbaF32::WHITE]),
    };
    let gradients = [gradient];
    let mut table = RowTable::default();
    let mut resolver = GradientResolver::default();
    let brush = LoweredBrush::Gradient {
        id: GradientId(0),
        hash: ContentHash::default(),
    };

    let mut pass = resolver.begin(&gradients, &mut table);
    let first = pass.source(brush);
    let registered = pass.registrations();
    let repeated = pass.source(brush);
    assert_eq!(pass.registrations(), registered);
    match (first, repeated) {
        (BrushSource::Gradient(first), BrushSource::Gradient(repeated)) => {
            assert_eq!(first.axis, repeated.axis);
            assert_eq!(first.kind, repeated.kind);
            assert_eq!(first.lut_row, repeated.lut_row);
        }
        _ => panic!("gradient brush resolved to a solid source"),
    }

    let mut pass = resolver.begin(&gradients, &mut table);
    assert!(!pass.is_resolved(GradientId(0)));
    let _ = pass.source(brush);
    assert_eq!(pass.registrations(), registered + 1);
}

/// Baseline counts: an empty tree emits no draws, a filled Frame one rect quad; invisible Frames, degenerate
/// Chrome and clip-only Surfaces emit none.
#[test]
fn baseline_draw_rect_count_cases() {
    #[derive(Debug)]
    enum Scene {
        Empty,
        FrameWithFill,
        InvisibleFrame,
        FrameWithDegenerateBg,
        FrameWithClipRectSurface,
    }
    let cases: &[(&str, Scene, usize)] = &[
        ("empty_tree", Scene::Empty, 0),
        ("frame_with_fill", Scene::FrameWithFill, 1),
        ("invisible_frame", Scene::InvisibleFrame, 0),
        (
            "frame_with_degenerate_background",
            Scene::FrameWithDegenerateBg,
            0,
        ),
        (
            "frame_with_clip_rect_surface",
            Scene::FrameWithClipRectSurface,
            0,
        ),
    ];
    for (label, scene, expected) in cases {
        let mut h = Harness::mono(UVec2::new(200, 200));
        h.frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| match scene {
                Scene::Empty => {}
                Scene::FrameWithFill => {
                    Block::new()
                        .id(WidgetId::from_hash("a"))
                        .size(50.0)
                        .chrome(Chrome::new(RgbaF32::srgb(1.0, 0.0, 0.0)))
                        .show(ui);
                }
                Scene::InvisibleFrame => {
                    Block::new()
                        .id(WidgetId::from_hash("invisible"))
                        .size(50.0)
                        .show(ui);
                }
                Scene::FrameWithDegenerateBg => {
                    Block::new()
                        .id(WidgetId::from_hash("degenerate"))
                        .size(50.0)
                        .chrome(
                            Chrome::NONE
                                .with_fill(RgbaF32::TRANSPARENT)
                                .with_border(Border::NONE),
                        )
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

/// The encoder emits every shape variant, not just `Text`.
#[test]
fn manually_pushed_shapes_emit_expected_cmds() {
    use crate::primitives::paint::lut_row::LutRow;
    use crate::shape::Shape;

    let tint = RgbaF32::new(0.5, 0.25, 1.0, 0.5);
    let mut h = Harness::mono(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            ui.add_shape(Shape::rect().corners(4.0), RgbaF32::srgb(1.0, 0.0, 0.0));
            ui.add_shape(Shape::rect().corners(6.0), RgbaF32::srgb(0.0, 1.0, 0.0));
            ui.add_shape(
                Shape::line(Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0)),
                Stroke::new(RgbaF32::srgb(1.0, 0.0, 0.0), 2.0),
            );
            ui.add_shape(
                Shape::line(Vec2::new(0.0, 5.0), Vec2::new(20.0, 5.0)),
                Stroke::new(tint, 2.0).with_ramp(ColorRamp::from([RgbaF32::BLACK, RgbaF32::WHITE])),
            );
            ui.add_shape(
                Shape::line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0)),
                Stroke::new(tint, 2.0).with_ramp(ColorRamp::from([
                    RgbaF32::TRANSPARENT,
                    RgbaF32::TRANSPARENT,
                ])),
            );
            ui.add_shape(
                Shape::line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0)),
                Stroke::new(RgbaF32::srgb(1.0, 0.0, 0.0), 0.0),
            );
            ui.add_shape(
                Shape::line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0)),
                Stroke::new(RgbaF32::TRANSPARENT, 2.0),
            );
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
        .filter_map(|command| as_rect(command))
        .map(|p| p.paint.fill.kind)
        .collect();
    assert_eq!(
        rect_kinds,
        [FillKind::SOLID, FillKind::SOLID],
        "both rects fill solid",
    );
    // A Line rides the GPU curve pipeline as a degenerate cubic, never a polyline.
    assert_eq!(cmds.kinds(), ["Fill", "Fill", "Stroke", "Stroke"]);
    let strokes = [2, 3].map(|i| cmds.calls[i].as_stroke().unwrap());
    assert!(
        strokes
            .iter()
            .all(|p| matches!(p.path, StrokePath::Curve(CurveBasis::Cubic(_)))),
        "{strokes:?}"
    );
    let [solid, ramp] = strokes.map(|p| p.fill);
    assert_eq!(
        (solid.kind, solid.color),
        (FillKind::SOLID, RgbaF32::srgb(1.0, 0.0, 0.0).to_f16()),
    );
    assert_eq!((ramp.kind, ramp.color), (FillKind::RAMP, tint.to_f16()));
    assert_ne!(
        ramp.lut_row,
        LutRow::FALLBACK,
        "the ramp resolved to a baked row"
    );
    assert_eq!(
        h.ui().forest().record_store.polyline_points.len(),
        0,
        "the point payloads stay untouched by lines"
    );
}

/// Both shadow kinds lower to their source rect and `(offset, σ, spread)` lanes.
#[test]
fn shadows_lower_to_their_source_and_geometry_lanes() {
    use crate::primitives::paint::shadow::Shadow;

    use crate::shape::Shape;

    let mut h = Harness::mono(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            ui.add_shape(
                Shape::rect()
                    .at(Rect::new(10.0, 20.0, 30.0, 40.0))
                    .corners(4.0),
                Shadow {
                    color: RgbaF32::srgba(0.0, 0.0, 0.0, 0.5),
                    offset: Vec2::new(2.0, 4.0),
                    blur: 8.0,
                    spread: -1.0,
                    inset: false,
                },
            );
            ui.add_shape(
                Shape::rect()
                    .at(Rect::new(10.0, 20.0, 30.0, 40.0))
                    .corners(4.0),
                Shadow {
                    color: RgbaF32::srgba(0.0, 0.0, 0.0, 0.5),
                    offset: Vec2::new(2.0, 4.0),
                    blur: 8.0,
                    spread: -2.0,
                    inset: true,
                },
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
    let (drop_rect, inset_rect) = (drop.source.rect, inset.source.rect);

    assert!(!drop.shadow.inset());
    assert_eq!(drop_rect, inset_rect, "both carry the one source rect");
    assert_eq!(drop.shadow.geometry_f16.lanes(), [2.0, 4.0, 8.0, -1.0]);
    assert_eq!(
        drop.shadow.color,
        RgbaF32::srgba(0.0, 0.0, 0.0, 0.5).to_f16()
    );

    assert!(inset.shadow.inset());
    assert_eq!(inset_rect.size, Size::new(30.0, 40.0));
    assert_eq!(inset.shadow.geometry_f16.lanes(), [2.0, 4.0, 8.0, -2.0]);
}

/// Chrome paints shadows in CSS Backgrounds 3 §7.1 order: drop under the fill, inset over it. An inset
/// shadow's source is the padding box on the padding edge's radii: fitted to the rect (§5.5), then each
/// `max(r − w, 0)` (§5.2); the width is the layout fold's, so a transparent border still moves it in.
/// A rect shape of the same chrome over the same rect paints the same two quads in the same order.
#[test]
fn chrome_shadow_paints_drop_under_and_inset_over_the_fill() {
    use crate::primitives::geometry::corners::Corners;

    use crate::primitives::paint::shadow::Shadow;
    use crate::shape::Shape;
    use crate::shape::geometry::box_geometry::PlacedBox;

    struct Case {
        label: &'static str,
        inset: bool,
        border: Border,
        corners: Corners,
        shadow_rect: Rect,
        shadow_corners: Corners,
    }
    let red = RgbaF32::srgb(1.0, 0.0, 0.0);
    let cases = [
        Case {
            label: "drop",
            inset: false,
            border: Border::new(red, 2.0),
            corners: Corners::all(8.0),
            shadow_rect: Rect::new(0.0, 0.0, 100.0, 60.0),
            shadow_corners: Corners::all(8.0),
        },
        Case {
            label: "inset under a 2 px border",
            inset: true,
            border: Border::new(red, 2.0),
            // 8 − 2 = 6; 1 − 2 and 0 − 2 floor at 0; 3 − 2 = 1.
            corners: Corners::new(8.0, 1.0, 0.0, 3.0),
            shadow_rect: Rect::new(2.0, 2.0, 96.0, 56.0),
            shadow_corners: Corners::new(6.0, 0.0, 0.0, 1.0),
        },
        Case {
            label: "inset under radii that overlap",
            inset: true,
            border: Border::new(red, 2.0),
            // The left side's 100 + 100 fit its 60 px at f = 0.3 before the border comes off: 30 − 2 = 28, 20·0.3 − 2 = 4.
            corners: Corners::new(100.0, 20.0, 20.0, 100.0),
            shadow_rect: Rect::new(2.0, 2.0, 96.0, 56.0),
            shadow_corners: Corners::new(28.0, 4.0, 4.0, 28.0),
        },
        Case {
            label: "inset under a transparent 4 px border",
            inset: true,
            border: Border::new(RgbaF32::TRANSPARENT, 4.0),
            corners: Corners::all(8.0),
            shadow_rect: Rect::new(4.0, 4.0, 92.0, 52.0),
            shadow_corners: Corners::all(4.0),
        },
        Case {
            label: "inset with no border",
            inset: true,
            border: Border::NONE,
            corners: Corners::all(8.0),
            shadow_rect: Rect::new(0.0, 0.0, 100.0, 60.0),
            shadow_corners: Corners::all(8.0),
        },
    ];
    for case in &cases {
        let shadow_of = if case.inset {
            Shadow::inset
        } else {
            Shadow::drop
        };
        let shadow = shadow_of(RgbaF32::srgba(0.0, 0.0, 0.0, 0.5), Vec2::new(0.0, 2.0), 6.0);
        let chrome = Chrome {
            corners: case.corners,
            paint: Paint {
                fill: RgbaF32::WHITE.into(),
                border: case.border,
                shadow,
            },
        };
        for entry in ["chrome", "rect shape"] {
            let mut h = Harness::mono(UVec2::new(200, 200));
            h.frame(|ui| {
                Panel::hstack().auto_id().show(ui, |ui| {
                    if entry == "chrome" {
                        Block::new()
                            .id(WidgetId::from_hash("chrome"))
                            .size((100.0, 60.0))
                            .chrome(chrome.clone())
                            .show(ui);
                    } else {
                        Panel::canvas()
                            .id(WidgetId::from_hash("chrome"))
                            .size((100.0, 60.0))
                            .show(ui, |ui| {
                                ui.add_shape(
                                    Shape::rect().corners(chrome.corners),
                                    chrome.paint.clone(),
                                );
                            });
                    }
                });
            });
            let cmds = h.encode_paint();
            let quads: Vec<_> = cmds
                .calls
                .iter()
                .filter(|c| as_rect(c).is_some() || as_shadow(c).is_some())
                .collect();
            let label = format!("{entry}: {}", case.label);
            assert_eq!(quads.len(), 2, "{label}: one fill and one shadow");
            let (fill, shadow) = if case.inset {
                (as_rect(quads[0]), as_shadow(quads[1]))
            } else {
                (as_rect(quads[1]), as_shadow(quads[0]))
            };
            let fill = fill.unwrap_or_else(|| panic!("{label}: fill out of order"));
            let shadow = shadow.unwrap_or_else(|| panic!("{label}: shadow out of order"));
            let node = quad_rect(fill);
            assert_eq!(node.size, Size::new(100.0, 60.0), "{label}");
            assert_eq!(shadow.shadow.inset(), case.inset, "{label}");
            let PlacedBox { rect, corners } = shadow.source;
            let expected = Rect {
                min: node.min + case.shadow_rect.min,
                size: case.shadow_rect.size,
            };
            assert_eq!(rect, expected, "{label}");
            assert_eq!(corners, case.shadow_corners, "{label}");
            assert_eq!(
                shadow.shadow.geometry_f16.lanes(),
                [0.0, 2.0, 6.0, 0.0],
                "{label}"
            );
        }
    }
}

#[test]
fn text_shape_carries_source_without_reconstructing_buffer() {
    use crate::widgets::text::Text;
    fn body(ui: &mut Ui) {
        Panel::hstack().auto_id().show(ui, |ui| {
            Text::new("hi").auto_id().show(ui);
        });
    }

    let mut h = Harness::new(UVec2::new(200, 200));
    h.frame(body);
    let key = h.ui().plane_layout(Layer::Main).text_shapes[0].buffer_key();
    h.ui().shaper().drop_cosmic_buffers();
    assert!(
        !h.ui().shaper().has_cosmic_buffer(key),
        "fixture must evict the retained layout's key",
    );

    let cmds = h.encode_paint();
    assert_eq!(cmds.kinds(), ["Text"]);
    let payload = cmds.calls[0].as_text().unwrap();
    let scene = h.frames().ui().frame_scene();
    let interned_text = scene.forest.record_store.text.interned();
    assert_eq!(interned_text.resolve(payload.text.span), "hi");
    assert!(
        !h.ui().shaper().has_cosmic_buffer(key),
        "frontend encoding must not reconstruct an evicted text buffer",
    );

    h.ui().shaper().drop_cosmic_buffers();
    let measure_calls = h.ui().shaper().measure_calls();
    h.ui_mut().request_repaint();
    h.frame(body);
    let replayed_key = h.ui().plane_layout(Layer::Main).text_shapes[0].buffer_key();
    assert_eq!(replayed_key, key);
    assert_eq!(
        h.ui().shaper().measure_calls(),
        measure_calls,
        "unchanged full record must replay text layout without reshaping",
    );
    assert!(
        !h.ui().shaper().has_cosmic_buffer(replayed_key),
        "layout replay must be allowed to retain an evicted cache key",
    );
    let replayed = h.encode_paint();
    assert_eq!(replayed.kinds(), ["Text"], "replayed text must still emit");
    let payload = replayed.calls[0].as_text().unwrap();
    let scene = h.frames().ui().frame_scene();
    let interned_text = scene.forest.record_store.text.interned();
    assert_eq!(interned_text.resolve(payload.text.span), "hi");
    assert!(
        !h.ui().shaper().has_cosmic_buffer(replayed_key),
        "frontend replay must leave reconstruction to an encoded-cache miss",
    );
}

#[test]
fn encoder_text_alignment_respects_leaf_padding() {
    use crate::widgets::button::Button;

    let mut h = Harness::new(UVec2::new(400, 400)).with_fixture_font_size();
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
        ["Fill", "Text"],
        "the button's chrome, then its label"
    );
    let text_rect = cmds.calls[1].as_text().unwrap().rect;

    // "ok" is 19 px in the bundled face and one 20 px line, centered on
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
