use crate::internals::ui_harness::UiHarness;
use crate::internals::ui_harness::size_trio::SizeTrio;
use crate::primitives::geometry::size::Size;
use std::f32::consts::TAU;

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::theme::spinner::SpinnerTheme;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::spinner::Spinner;
use crate::widgets::spinner::{ArcGeometry, comet};
use glam::UVec2;
use glam::Vec2;
use std::time::Duration;

/// The trace circle insets by half the stroke width (round caps), and degenerate sizes clamp at zero.
#[test]
fn arc_geometry_insets_by_half_width() {
    assert_eq!(
        ArcGeometry::new(24.0, 2.0),
        ArcGeometry {
            center: Vec2::splat(12.0),
            radius: 11.0,
        }
    );
    assert_eq!(ArcGeometry::new(4.0, 8.0).radius, 0.0);
    // The default sweep leaves a gap; a full turn would read as a static ring.
    assert!(SpinnerTheme::default().sweep < TAU);
}

/// Sweep, period and stroke come from `Theme::spinner`. Stroke is `diameter * width_ratio` floored at `min_width`.
#[test]
fn arc_and_spin_follow_the_spinner_theme() {
    use crate::primitives::geometry::circular_arc::CircularArc;
    use crate::shape::paint::curve_basis::CurveBasis;
    use crate::shape::record::ShapeRecord;

    #[derive(Debug)]
    struct Recorded {
        sweep: f32,
        width: f32,
        period: Duration,
    }

    fn recorded(theme: SpinnerTheme, diameter: f32, width: Option<f32>) -> Recorded {
        let mut h = UiHarness::new(UVec2::new(200, 200));
        h.ui.theme_mut().spinner = theme;
        h.frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                let spinner = Spinner::new()
                    .id(WidgetId::from_hash("spin"))
                    .diameter(diameter);
                match width {
                    Some(px) => spinner.width(px),
                    None => spinner,
                }
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
                    basis: CurveBasis::Arc(CircularArc { sweep, .. }),
                    stroke,
                    ..
                } => Some((*sweep, stroke.width)),
                _ => None,
            })
            .expect("spinner records one arc");
        let period = tree
            .paint_animations
            .entries
            .iter()
            .find_map(|e| e.animation.channel.turn.map(|_| e.animation.timing.period))
            .expect("spinner registers a turning anim");
        Recorded {
            sweep: arc.0,
            width: arc.1,
            period,
        }
    }

    let stock = SpinnerTheme::default();
    let Recorded {
        sweep,
        width,
        period,
    } = recorded(stock.clone(), 50.0, None);
    assert_eq!(sweep, stock.sweep, "sweep is themed");
    assert_eq!(period, stock.period, "the period is themed");
    let expected = 50.0 * stock.width_ratio;
    assert_eq!(width, expected, "want {expected}, got {width}");

    let small = recorded(stock.clone(), 12.5, None).width;
    let expected_small = 12.5 * stock.width_ratio;
    assert_eq!(small, expected_small);
    assert_ne!(width, small);

    let tiny = stock.min_width / stock.width_ratio * 0.5;
    let floored = recorded(stock.clone(), tiny, None).width;
    assert_eq!(
        floored, stock.min_width,
        "tiny spinner floors at min_width, got {floored}"
    );

    // An explicit width replaces the derived one, floor included.
    let explicit = stock.min_width * 0.5;
    assert_eq!(recorded(stock, 50.0, Some(explicit)).width, explicit);

    let loud = SpinnerTheme {
        sweep: 1.0,
        period: Duration::from_millis(700),
        width_ratio: 0.5,
        ..SpinnerTheme::default()
    };
    let Recorded {
        sweep: sweep_b,
        width: width_b,
        period: period_b,
    } = recorded(loud, 50.0, None);
    assert_eq!(sweep_b, 1.0);
    assert_eq!(period_b, Duration::from_millis(700));
    assert_eq!(width_b, 25.0);
    assert_ne!(sweep, sweep_b);
    assert_ne!(period, period_b);
    assert_ne!(width, width_b);
}

/// Comet trail: transparent white tail, opaque white head, so the stroke color sets the hue; a translucent base keeps its alpha at the head.
#[test]
fn comet_fades_tail_to_head() {
    let ramp = comet();
    assert_eq!(ramp.stops.len(), 2);
    let (tail, head) = (ramp.stops[0], ramp.stops[1]);
    assert_eq!((tail.offset(), head.offset()), (0.0, 1.0));
    assert_eq!(tail.color(), RgbaF32::new(1.0, 1.0, 1.0, 0.0));
    assert_eq!(head.color(), RgbaF32::WHITE);

    let base = RgbaF32::srgb(0.6, 0.8, 1.0).with_alpha(0.5);
    assert_eq!(base.tinted(head.color()), base);
    assert_eq!(base.tinted(tail.color()), base.with_alpha(0.0));
}

/// Layout size and drawn diameter are separate: an explicit size or `HUG` replaces the box; default is the diameter square.
#[test]
fn explicit_layout_size_is_independent_from_diameter() {
    let trio = SizeTrio::of((Sizing::fixed(30.0), Sizing::fixed(40.0)), |ui, size| {
        let mut spinner = Spinner::new().diameter(12.0);
        if let Some(size) = size {
            spinner = spinner.size(size);
        }
        spinner.show(ui).node()
    });
    assert_eq!(
        trio,
        SizeTrio {
            sized: Size::new(30.0, 40.0),
            hug: Size::ZERO,
            default: Size::new(12.0, 12.0),
        }
    );
}
