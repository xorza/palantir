use crate::host::harness::Harness;
use crate::internals::size_trio::SizeTrio;
use crate::primitives::geometry::size::Size;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme;
use std::f32::consts::TAU;

use crate::animation::motion_role::MotionRole;
use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::visibility::Visibility;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::shape::Shape;
use crate::shape::paint::arc_geometry::ArcGeometry;
use crate::shape::paint_animation::{PaintAnimation, PaintRepeat};
use crate::shape::record::ShapeRecord;
use crate::theme::spinner::SpinnerTheme;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::spinner::Spinner;
use crate::widgets::spinner::{SpinnerCircle, comet};
use glam::UVec2;
use glam::Vec2;
use std::time::Duration;

/// The trace circle insets by half the stroke width (round caps), sits at the center of its
/// box, and degenerate sizes clamp at zero.
#[test]
fn spinner_circle_insets_by_half_width() {
    assert_eq!(
        SpinnerCircle::new(Vec2::splat(24.0), 24.0, 2.0),
        SpinnerCircle {
            center: Vec2::splat(12.0),
            radius: 11.0,
        }
    );
    assert_eq!(
        SpinnerCircle::new(Vec2::new(30.0, 40.0), 30.0, 2.0),
        SpinnerCircle {
            center: Vec2::new(15.0, 20.0),
            radius: 14.0,
        }
    );
    assert_eq!(SpinnerCircle::new(Vec2::splat(4.0), 4.0, 8.0).radius, 0.0);
    // The default sweep leaves a gap; a full turn would read as a static ring.
    assert!(Theme::stock(ColorScheme::Dark).spinner.sweep < TAU);
}

/// Sweep, period and stroke come from `Theme::spinner`. The stroke width is `diameter * width_ratio` floored at `min_stroke_width`.
#[test]
fn arc_and_spin_follow_the_spinner_theme() {
    #[derive(Debug)]
    struct Recorded {
        sweep: f32,
        width: f32,
        period: Duration,
    }

    fn recorded(theme: SpinnerTheme, diameter: f32, width: Option<f32>) -> Recorded {
        let mut h = Harness::mono(UVec2::new(200, 200));
        h.ui_mut().theme_mut().spinner = theme;
        h.frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                let spinner = Spinner::new()
                    .id(WidgetId::from_hash("spin"))
                    .size(diameter);
                match width {
                    Some(px) => spinner.width(px),
                    None => spinner,
                }
                .show(ui);
            });
        });
        let tree = h.ui().tree(Layer::Main);
        let arc = tree
            .shapes
            .records
            .iter()
            .find_map(|s| match s {
                ShapeRecord::Arc {
                    geometry: ArcGeometry { sweep, .. },
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

    let stock = Theme::stock(ColorScheme::Dark).spinner;
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

    let tiny = stock.min_stroke_width / stock.width_ratio * 0.5;
    let floored = recorded(stock.clone(), tiny, None).width;
    assert_eq!(
        floored, stock.min_stroke_width,
        "tiny spinner floors at min_stroke_width, got {floored}"
    );

    // An explicit width replaces the derived one, floor included.
    let explicit = stock.min_stroke_width * 0.5;
    assert_eq!(recorded(stock, 50.0, Some(explicit)).width, explicit);

    let loud = SpinnerTheme {
        sweep: 1.0,
        period: Duration::from_millis(700),
        width_ratio: 0.5,
        ..Theme::stock(ColorScheme::Dark).spinner
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

/// A spinner sizes as a leaf: an explicit size or `HUG` replaces the box, and the default is
/// the theme's diameter square. A fixed size draws the arc across its shorter side, centered:
/// 30 × 40 draws a 30 px arc around (15, 20); `HUG` draws the theme's.
#[test]
fn a_fixed_size_sets_the_box_and_the_arc() {
    let stock = Theme::stock(ColorScheme::Dark).spinner;
    let trio = SizeTrio::of((Sizing::fixed(30.0), Sizing::fixed(40.0)), |ui, size| {
        let mut spinner = Spinner::new();
        if let Some(size) = size {
            spinner = spinner.size(size);
        }
        spinner.show(ui).node(ui)
    });
    assert_eq!(
        trio,
        SizeTrio {
            sized: Size::new(30.0, 40.0),
            hug: Size::ZERO,
            default: Size::new(stock.diameter, stock.diameter),
        }
    );

    let arc = |size: (Sizing, Sizing)| {
        let mut h = Harness::mono(UVec2::new(200, 200));
        h.frame(|ui| {
            Spinner::new()
                .id(WidgetId::from_hash("spin"))
                .size(size)
                .show(ui);
        });
        h.ui()
            .tree(Layer::Main)
            .shapes
            .records
            .iter()
            .find_map(|s| match s {
                ShapeRecord::Arc {
                    geometry:
                        ArcGeometry {
                            local_rect: Some(rect),
                            start_angle,
                            sweep,
                        },
                    ..
                } => Some(CircularArc::inscribed(*rect, *start_angle, *sweep)),
                _ => None,
            })
            .expect("spinner records one arc")
    };
    let width = (30.0 * stock.width_ratio).max(stock.min_stroke_width);
    let fixed = arc((Sizing::fixed(30.0), Sizing::fixed(40.0)));
    assert_eq!(fixed.center, Vec2::new(15.0, 20.0));
    assert_eq!(fixed.radius, (30.0 - width) * 0.5);
    let hug = arc((Sizing::HUG, Sizing::HUG));
    let theme_width = (stock.diameter * stock.width_ratio).max(stock.min_stroke_width);
    assert_eq!(hug.radius, (stock.diameter - theme_width) * 0.5);
}

#[derive(Clone, Copy, Debug)]
enum InvisibleSpinnerCase {
    Hidden,
    Collapsed,
    HiddenAncestor,
}

fn show_spinner_case(ui: &mut Ui, case: InvisibleSpinnerCase) {
    match case {
        InvisibleSpinnerCase::Hidden => {
            Spinner::new()
                .id(WidgetId::from_hash("spinner"))
                .hidden()
                .show(ui);
        }
        InvisibleSpinnerCase::Collapsed => {
            Spinner::new()
                .id(WidgetId::from_hash("spinner"))
                .collapsed()
                .show(ui);
        }
        InvisibleSpinnerCase::HiddenAncestor => {
            Panel::hstack()
                .id(WidgetId::from_hash("hidden-parent"))
                .hidden()
                .show(ui, |ui| {
                    Spinner::new().id(WidgetId::from_hash("spinner")).show(ui);
                });
        }
    }
}

#[test]
fn effectively_invisible_spinners_keep_their_shape_without_scheduling_frames() {
    for case in [
        InvisibleSpinnerCase::Hidden,
        InvisibleSpinnerCase::Collapsed,
        InvisibleSpinnerCase::HiddenAncestor,
    ] {
        let mut h = Harness::mono(UVec2::new(100, 100));
        let report = h.frame(|ui| {
            show_spinner_case(ui, case);
        });
        let tree = h.ui().tree(Layer::Main);

        assert_eq!(
            tree.shapes.records.len(),
            1,
            "{case:?}: the authored spinner shape must survive",
        );
        assert!(
            tree.paint_animations.entries.is_empty(),
            "{case:?}: an invisible spinner must have no active animation row",
        );
        assert_eq!(
            report.repaint_at, None,
            "{case:?}: an invisible spinner must not schedule another frame",
        );
    }
}

#[test]
fn spinner_animation_stops_when_hidden_and_resumes_when_shown() {
    fn show_spinner(ui: &mut Ui, visibility: Visibility) {
        Spinner::new()
            .id(WidgetId::from_hash("transition-spinner"))
            .visibility(visibility)
            .show(ui);
    }

    let mut h = Harness::mono(UVec2::new(100, 100));

    let visible = h.frame(|ui| {
        show_spinner(ui, Visibility::Visible);
    });
    assert_eq!(visible.repaint_at, Some(Duration::ZERO));
    assert_eq!(h.ui().tree(Layer::Main).paint_animations.entries.len(), 1);

    h.ui_mut().request_repaint();
    let hidden_at = Duration::from_millis(16);
    let hidden = h.at(hidden_at).frame(|ui| {
        show_spinner(ui, Visibility::Hidden);
    });
    assert_eq!(hidden.repaint_at, None);
    assert_eq!(
        h.ui().tree(Layer::Main).shapes.records.len(),
        1,
        "hiding must retain the authored spinner shape",
    );
    assert!(
        h.ui().tree(Layer::Main).paint_animations.entries.is_empty(),
        "hiding must drop the active animation row",
    );

    h.ui_mut().request_repaint();
    let shown_at = Duration::from_millis(32);
    let shown = h.at(shown_at).frame(|ui| {
        show_spinner(ui, Visibility::Visible);
    });
    assert_eq!(shown.repaint_at, Some(shown_at));
    assert_eq!(
        h.ui().tree(Layer::Main).paint_animations.entries.len(),
        1,
        "showing must restore the active animation row",
    );
}

/// A spinner out of view keeps its row but schedules no frame, as its turns paint nothing:
/// clipped by a scroll viewport, and past the surface edge with no clip on its branch. A
/// scroll that shows it records, and that frame schedules its turn at once.
#[test]
fn a_spinner_out_of_view_schedules_no_frame() {
    let scroll = WidgetId::from_hash("scroll");
    let in_scroll = |ui: &mut Ui| {
        Scroll::vertical()
            .id(scroll)
            .size((Sizing::fixed(100.0), Sizing::fixed(50.0)))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("above"))
                    .size((Sizing::fixed(100.0), Sizing::fixed(200.0)))
                    .show(ui);
                Spinner::new().id(WidgetId::from_hash("spinner")).show(ui);
            });
    };
    let mut h = Harness::mono(UVec2::new(100, 100));
    let clipped = h.frame(in_scroll);
    assert_eq!(
        h.ui().tree(Layer::Main).paint_animations.entries.len(),
        1,
        "a clipped spinner keeps its row",
    );
    assert_eq!(clipped.repaint_at, None, "the viewport clips the spinner");

    Scroll::set_offset(h.ui_mut(), scroll, Vec2::new(0.0, 200.0));
    h.ui_mut().request_repaint();
    let shown_at = Duration::from_millis(16);
    let shown = h.at(shown_at).frame(in_scroll);
    assert_eq!(shown.repaint_at, Some(shown_at), "scrolled into view");

    let mut h = Harness::mono(UVec2::new(100, 100));
    let below = h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("column"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("above"))
                    .size((Sizing::fixed(100.0), Sizing::fixed(150.0)))
                    .show(ui);
                Spinner::new().id(WidgetId::from_hash("spinner")).show(ui);
            });
    });
    assert_eq!(
        h.ui().tree(Layer::Main).paint_animations.entries.len(),
        1,
        "a spinner past the surface edge keeps its row",
    );
    assert_eq!(below.repaint_at, None, "the surface edge hides the spinner");
}

/// Under reduced motion a spinner's turn shows progress and keeps running, and a decorative turn
/// recorded beside it paints as if unanimated.
#[test]
fn a_spinner_turns_under_reduced_motion_and_decoration_does_not() {
    let mut h = Harness::mono(UVec2::new(200, 200));
    h.set_system_motion_reduced(true);
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Spinner::new().id(WidgetId::from_hash("spin")).show(ui);
            Panel::zstack()
                .id(WidgetId::from_hash("decor"))
                .size(10.0)
                .show(ui, |ui| {
                    if let Some(shape) = ui.add_shape(Shape::rect(), RgbaF32::WHITE) {
                        shape.animate(
                            PaintAnimation::turn(0.0, 1.0).with_repeat(PaintRepeat::Forever),
                        );
                    }
                });
        });
    });
    let motions: Vec<MotionRole> = h
        .ui()
        .tree(Layer::Main)
        .paint_animations
        .entries
        .iter()
        .map(|e| e.animation.motion)
        .collect();
    assert_eq!(motions, [MotionRole::Essential]);
}
