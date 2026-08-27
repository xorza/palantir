use crate::ui::harness::UiHarness;
use std::f32::consts::TAU;

use crate::layout::types::sizing::Sizing;
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::spinner::Spinner;
use crate::widgets::spinner::{ArcGeometry, arc_geometry, comet_brush};
use crate::widgets::theme::spinner::SpinnerTheme;
use glam::UVec2;
use glam::Vec2;

/// The trace circle insets by half the stroke width (round caps
/// reach `width/2` past the centerline, so this keeps the painted
/// stroke inside the box), and degenerate sizes clamp at zero.
#[test]
fn arc_geometry_insets_by_half_width() {
    assert_eq!(
        arc_geometry(24.0, 2.0),
        ArcGeometry {
            center: Vec2::splat(12.0),
            radius: 11.0,
        }
    );
    // width ≥ size: radius clamps to 0 instead of going negative.
    assert_eq!(arc_geometry(4.0, 8.0).radius, 0.0);
    // The default sweep leaves a visible gap — a full turn would
    // paint as a static ring, with nothing to read the spin off.
    assert!(SpinnerTheme::default().sweep < TAU);
}

/// Sweep, spin rate, and the diameter-derived stroke all come off
/// `Theme::spinner` rather than constants. Stroke is
/// `diameter * thickness_ratio` floored at `min_thickness`, so the
/// arc keeps its proportions when the spinner is resized — and the
/// floor is what a tiny one lands on.
#[test]
fn arc_and_spin_follow_the_spinner_theme() {
    use crate::scene::shapes::paint::CurveBasis;
    use crate::scene::shapes::record::ShapeRecord;
    use crate::scene::tree::paint_anims::PaintAnim;

    fn recorded(theme: SpinnerTheme, diameter: f32) -> (f32, f32, f32) {
        let mut h = UiHarness::new(UVec2::new(200, 200));
        h.ui.theme_mut().spinner = theme;
        h.frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Spinner::new()
                    .id(WidgetId::from_hash("spin"))
                    .diameter(diameter)
                    .show(ui);
            });
        });
        let tree = h.ui.tree(Layer::Main);
        let arc = tree
            .shapes
            .records
            .iter()
            .find_map(|s| match s {
                ShapeRecord::Curve {
                    basis: CurveBasis::Arc { a1, .. },
                    width,
                    ..
                } => Some((*a1, *width)),
                _ => None,
            })
            .expect("spinner records one arc");
        let speed = tree
            .paint_anims
            .entries
            .iter()
            .find_map(|e| match e.anim {
                PaintAnim::Spin { speed, .. } => Some(speed),
                _ => None,
            })
            .expect("spinner registers a spin anim");
        (arc.0, arc.1, speed)
    }

    // Stock theme: stroke is the ratio applied to the diameter,
    // clear of the floor at 50 px.
    let stock = SpinnerTheme::default();
    let (sweep, width, speed) = recorded(stock.clone(), 50.0);
    assert!((sweep - stock.sweep).abs() < 1e-4, "sweep is themed");
    assert!((speed - stock.speed).abs() < 1e-4, "spin rate is themed");
    let expected = 50.0 * stock.thickness_ratio;
    assert!(
        (width - expected).abs() < 1e-4,
        "want {expected}, got {width}"
    );

    // Quarter the diameter and the stroke follows it down, rather
    // than staying put.
    let (_, small, _) = recorded(stock.clone(), 12.5);
    let expected_small = 12.5 * stock.thickness_ratio;
    assert!((small - expected_small).abs() < 1e-4);
    assert_ne!(width, small);

    // Below the floor the derived value loses.
    let tiny = stock.min_thickness / stock.thickness_ratio * 0.5;
    let (_, floored, _) = recorded(stock.clone(), tiny);
    assert!(
        (floored - stock.min_thickness).abs() < 1e-4,
        "tiny spinner floors at min_thickness, got {floored}",
    );

    // Retheme: every one of the three moves.
    let loud = SpinnerTheme {
        sweep: 1.0,
        speed: 9.0,
        thickness_ratio: 0.5,
        ..SpinnerTheme::default()
    };
    let (sweep_b, width_b, speed_b) = recorded(loud, 50.0);
    assert!((sweep_b - 1.0).abs() < 1e-4);
    assert!((speed_b - 9.0).abs() < 1e-4);
    assert!((width_b - 25.0).abs() < 1e-4);
    assert_ne!(sweep, sweep_b);
    assert_ne!(speed, speed_b);
    assert_ne!(width, width_b);
}

/// Comet trail: tail transparent, head the full color, rgb equal on
/// both stops (only alpha fades). A translucent base scales — the
/// head must carry the base alpha, not opaque 1.0.
#[test]
fn comet_brush_fades_tail_to_head() {
    let base = Color::rgb(0.6, 0.8, 1.0).with_alpha(0.5);
    let g = comet_brush(base);
    assert_eq!(g.stops.len(), 2);
    let tail = g.stops[0];
    let head = g.stops[1];
    assert_eq!(tail.offset(), 0.0);
    assert_eq!(head.offset(), 1.0);
    assert_eq!(tail.color.a, 0);
    assert_eq!(head.color, ColorU8::from(base));
    // RGB is untouched — only alpha varies along the trail.
    assert_eq!(tail.color.r, head.color.r);
    assert_eq!(tail.color.g, head.color.g);
    assert_eq!(tail.color.b, head.color.b);
}

#[test]
fn explicit_layout_size_is_independent_from_diameter() {
    let mut h = UiHarness::new(UVec2::new(200, 120));
    let (mut sized, mut hug, mut default) = (None, None, None);
    h.frame(|ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            sized = Some(
                Spinner::new()
                    .diameter(12.0)
                    .size((Sizing::fixed(30.0), Sizing::fixed(40.0)))
                    .show(ui)
                    .node(),
            );
            hug = Some(
                Spinner::new()
                    .diameter(12.0)
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui)
                    .node(),
            );
            default = Some(Spinner::new().diameter(12.0).show(ui).node());
        });
    });

    let rects = &h.ui.layout(Layer::Main).rect;
    let sized = rects[sized.unwrap().idx()];
    let hug = rects[hug.unwrap().idx()];
    let default = rects[default.unwrap().idx()];
    assert_eq!((sized.size.w, sized.size.h), (30.0, 40.0));
    assert_eq!((hug.size.w, hug.size.h), (0.0, 0.0));
    assert_eq!((default.size.w, default.size.h), (12.0, 12.0));
}
