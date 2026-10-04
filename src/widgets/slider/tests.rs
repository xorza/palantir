use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::internals::harness::passes::Passes;
use crate::internals::harness::size_trio::SizeTrio;
use crate::internals::panic_probe;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::internals::ValueEdges;
use crate::widgets::panel::Panel;
use crate::widgets::slider::{Slider, fraction_to_value, snap_to_step, value_to_fraction};
use glam::{UVec2, Vec2};

/// One frame driven by a commit-deferring caller: the draft re-seeds
/// from `canonical` every record pass and is adopted only on
/// `committed`. One snapshot per record pass, since a one-frame edge only
/// shows in pass A and an undo pusher applies once per pass — so a commit
/// has to fire in exactly one of them.
fn deferred_frame(h: &mut UiHarness, id: WidgetId, canonical: &mut f64) -> Passes<ValueEdges> {
    h.frame_passes(|ui| {
        let mut draft = *canonical;
        let r = Slider::new(&mut draft, 0.0..=1.0)
            .size((Sizing::fixed(118.0), Sizing::fixed(18.0)))
            .id(id)
            .show(ui);
        if r.committed {
            *canonical = draft;
        }
        r.edges()
    })
}

/// The release frame re-writes the value, so a caller that re-seeds its
/// draft from a canonical copy every frame and adopts it only on
/// `committed` still observes the gesture's result. A release is neither
/// `pressed()` nor `dragging()`, so without naming it the deferred
/// caller would read its own seed back on the one frame it acts on.
///
/// Geometry: 118 wide, knob 18, so travel is 100 px starting at x = 9 —
/// x = 59 is fraction 0.5 and x = 89 is 0.8, straight through to the
/// value on an unstepped 0..=1 range.
#[test]
fn release_rewrites_the_value_once_for_a_deferred_caller() {
    let id = WidgetId::from_hash("slider-deferred-commit");
    let mut h = UiHarness::new(UVec2::new(118, 18));
    let mut canonical = 0.0_f64;

    // Settle a layout frame so the cascade exists for pointer routing.
    deferred_frame(&mut h, id, &mut canonical);

    h.press_at(Vec2::new(59.0, 9.0));
    let s = deferred_frame(&mut h, id, &mut canonical);
    assert!(
        s.a().changed && !s.a().committed,
        "press: live write, no commit"
    );
    assert_eq!(canonical, 0.0, "deferred caller ignores mid-drag writes");

    h.drag_to(Vec2::new(89.0, 9.0));
    let s = deferred_frame(&mut h, id, &mut canonical);
    assert!(
        s.a().changed && !s.a().committed,
        "drag: live write, no commit"
    );
    assert_eq!(canonical, 0.0);

    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical);
    assert!(s.a().committed, "release commits the gesture");
    assert_eq!(
        s.count_where(|e| e.committed),
        1,
        "one commit, one record pass"
    );
    assert_eq!(canonical, 0.8, "the commit frame carries the final value");

    let s = deferred_frame(&mut h, id, &mut canonical);
    assert!(!s.a().changed && !s.a().committed, "no residual signals");
    assert_eq!(canonical, 0.8);
}

/// A press and release on the track is a whole edit: it writes a value,
/// and it latches no drag, so a commit read off `drag.stopped()` never
/// fires for it. Every release ends a gesture, and every gesture owes one
/// commit.
///
/// Same geometry as the drag test above — 118 wide, knob 18, travel 100
/// from x = 9 — so x = 59 is 0.5 of an unstepped `0..=1` range.
#[test]
fn a_click_on_the_track_commits_the_value_it_wrote() {
    let id = WidgetId::from_hash("slider-click-commit");
    let mut h = UiHarness::new(UVec2::new(118, 18));
    let mut canonical = 0.0_f64;

    // Settle a layout frame so the cascade exists for pointer routing.
    deferred_frame(&mut h, id, &mut canonical);

    h.press_at(Vec2::new(59.0, 9.0));
    let s = deferred_frame(&mut h, id, &mut canonical);
    assert!(
        s.a().changed && !s.a().committed,
        "press: live write, no commit"
    );
    assert_eq!(
        canonical, 0.0,
        "deferred caller ignores the mid-gesture write"
    );

    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical);
    assert!(s.a().committed, "the click's release commits it");
    assert_eq!(
        s.count_where(|e| e.committed),
        1,
        "one commit, one record pass"
    );
    assert_eq!(
        canonical, 0.5,
        "the commit frame carries the value it wrote"
    );

    let s = deferred_frame(&mut h, id, &mut canonical);
    assert!(!s.a().changed && !s.a().committed, "no residual signals");
    assert_eq!(canonical, 0.5);
}

/// Explicit `.size(...)` wins over the widget's `Fill × knob_size`
/// default, and an untouched slider still gets that default
/// (400-wide FILL column → 400 × knob_size 18). Hugged, the slider is
/// the knob alone.
#[test]
fn explicit_size_overrides_fill_default() {
    let mut v = 0.5_f64;
    let trio = SizeTrio::of((Sizing::fixed(120.0), Sizing::fixed(30.0)), |ui, size| {
        let mut slider = Slider::new(&mut v, 0.0..=1.0);
        if let Some(size) = size {
            slider = slider.size(size);
        }
        slider.show(ui).response.node()
    });
    assert_eq!(
        trio,
        SizeTrio {
            sized: Size::new(120.0, 30.0),
            hug: Size::new(18.0, 18.0),
            default: Size::new(400.0, 18.0),
        }
    );
}

/// Each endpoint collapses one track segment to a zero-extent `Fixed`, and an
/// unseeded value lays out as the low end rather than reaching
/// `Sizing::share`'s finite assert — the value is app state the widget
/// borrows and cannot assert on.
#[test]
fn endpoint_rails_collapse_without_invalid_fill_weights() {
    for (value, expected) in [
        (0.0, [0.0, 18.0, 102.0]),
        (1.0, [102.0, 18.0, 0.0]),
        (f64::NAN, [0.0, 18.0, 102.0]),
    ] {
        let mut h = UiHarness::new(UVec2::new(120, 30));
        let mut value = value;
        let root = h.frame_value(|ui| {
            Slider::new(&mut value, 0.0..=1.0)
                .size((Sizing::fixed(120.0), Sizing::fixed(18.0)))
                .show(ui)
                .response
                .node()
        });
        let widths: Vec<_> = h
            .main_child_rects(root)
            .into_iter()
            .map(|rect| rect.size.w)
            .collect();
        assert_eq!(widths, expected, "value {value}");
    }
}

#[test]
fn value_to_fraction_maps_and_clamps() {
    let cases = [
        (50.0, 0.0, 100.0, 0.5),
        (0.0, 0.0, 100.0, 0.0),
        (100.0, 0.0, 100.0, 1.0),
        (150.0, 0.0, 100.0, 1.0), // above clamps
        (-10.0, 0.0, 100.0, 0.0), // below clamps
        (15.0, 10.0, 20.0, 0.5),  // offset range
        (5.0, 3.0, 3.0, 0.0),     // degenerate
        // The share is dimensionless, so the units it is taken in cannot
        // decide it: a range under the pixel tolerance and one past
        // `f32`'s reach both put their midpoint in the middle.
        (5e-6, 0.0, 1e-5, 0.5),
        (5e99, 0.0, 1e100, 0.5),
        (2.5e-7, 0.0, 1e-5, 0.025),
        // A share past `f32`'s reach is still a share, and it clamps to
        // the end it is past rather than to the low end.
        (1e300, 0.0, 1.0, 1.0),
        (-1e300, 0.0, 1.0, 0.0),
        (1.0, 0.0, 1e-300, 1.0),
        // A reversed range descends from left to right, and its midpoint
        // is still the middle of the track.
        (50.0, 100.0, 0.0, 0.5),
        (100.0, 100.0, 0.0, 0.0),
        (0.0, 100.0, 0.0, 1.0),
    ];
    for (v, min, max, want) in cases {
        let got = value_to_fraction(v, min, max);
        assert_eq!(got, want, "v2f({v},{min},{max})={got} want {want}");
    }
    // A NaN anywhere in the triple names no share, and the low end is
    // what this widget reads that as — the same answer `press_fraction`
    // gives a track with no travel.
    for (v, min, max) in [
        (f64::NAN, 0.0, 100.0),
        (50.0, f64::NAN, 100.0),
        (50.0, 0.0, f64::NAN),
    ] {
        assert_eq!(
            value_to_fraction(v, min, max),
            0.0,
            "v2f({v},{min},{max}) must read as the low end",
        );
    }
}

#[test]
fn fraction_to_value_inverts_value_to_fraction() {
    // Round-trip over an offset range.
    for &v in &[10.0_f64, 12.5, 15.0, 17.5, 20.0] {
        let f = value_to_fraction(v, 10.0, 20.0);
        let back = fraction_to_value(f, 10.0, 20.0);
        assert_eq!(back, v, "roundtrip {v} -> {f} -> {back}");
    }
    assert_eq!(fraction_to_value(0.25, 10.0, 20.0), 12.5);
    // A reversed range round-trips through the same inverse: 75 sits a
    // quarter of the way from 100 down to 0.
    let f = value_to_fraction(75.0, 100.0, 0.0);
    assert_eq!(f, 0.25, "reversed fraction {f}");
    assert_eq!(fraction_to_value(f, 100.0, 0.0), 75.0);
    // Out-of-range fraction clamps before mapping.
    assert_eq!(fraction_to_value(1.5, 0.0, 100.0), 100.0);
}

#[test]
fn pointer_mapping_is_scale_invariant() {
    let id = WidgetId::from_hash("scaled-slider");
    for scale in [0.5, 1.0, 2.0] {
        for (local_x, expected) in [(9.0, 0.0), (34.5, 0.25), (111.0, 1.0)] {
            let mut h = UiHarness::new(UVec2::new(300, 100));
            let mut value = 0.5_f64;
            let build = |ui: &mut Ui, value: &mut f64| {
                Panel::zstack()
                    .id(WidgetId::from_hash("scaled-slider-parent"))
                    .transform(TranslateScale::from_scale(scale))
                    .size((Sizing::fixed(120.0), Sizing::fixed(18.0)))
                    .show(ui, |ui| {
                        Slider::new(value, 0.0..=1.0)
                            .id(id)
                            .size((Sizing::fixed(120.0), Sizing::fixed(18.0)))
                            .show(ui);
                    });
            };
            h.frame(|ui| build(ui, &mut value));

            h.press_in(id, Vec2::new(local_x, 9.0));
            h.frame(|ui| build(ui, &mut value));

            assert_eq!(
                value, expected,
                "logical x={local_x} at {scale}× produced {value}, expected {expected}"
            );
        }
    }
}

#[test]
fn snap_to_step_rounds_to_grid() {
    assert_eq!(snap_to_step(53.0, 0.0, Some(10.0)), 50.0);
    assert_eq!(snap_to_step(57.0, 0.0, Some(10.0)), 60.0);
    assert_eq!(snap_to_step(12.0, 0.0, Some(5.0)), 10.0);
    assert_eq!(snap_to_step(13.0, 0.0, Some(5.0)), 15.0);
    // Off-anchor grid: steps of 0.5 from min=1.0.
    assert_eq!(snap_to_step(2.2, 1.0, Some(0.5)), 2.0);
    // A slider with no step passes the value through.
    assert_eq!(snap_to_step(53.0, 0.0, None), 53.0);
}

/// `None` is the only "off": the builder refuses a step that would be a
/// second spelling of it.
#[test]
fn step_rejects_a_value_that_cannot_snap() {
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        panic_probe::assert_panics_with("a positive value must be finite and above zero", || {
            let mut v = 0.5_f64;
            let _ = Slider::new(&mut v, 0.0..=1.0).step(bad);
        });
    }
}

/// A slider maps its track onto its range, so an infinite end is refused
/// where the slider is built: a click would otherwise store `+inf`, or
/// `NaN` over `-inf..=inf`.
#[test]
fn new_rejects_an_infinite_range() {
    for (lo, hi) in [
        (0.0, f64::INFINITY),
        (f64::NEG_INFINITY, 1.0),
        (f64::NAN, 1.0),
    ] {
        panic_probe::assert_panics_with("a range must have finite ends", || {
            let mut v = 0.5_f64;
            let _ = Slider::new(&mut v, lo..=hi);
        });
    }
}

/// The binding is `DragNum`, so the track drives an integer as readily
/// as a float, and every landing is whole.
///
/// Same geometry as the deferred-commit test: 118 wide, knob 18, so the
/// 100 px travel starts at x = 9. On a `0..=10` range x = 89 is 0.8 of
/// it — value 8 exactly — and x = 34 is 0.25, whose 2.5 rounds away from
/// zero.
#[test]
fn an_integer_target_lands_on_whole_values() {
    let id = WidgetId::from_hash("slider-int");
    let mut h = UiHarness::new(UVec2::new(118, 18));
    let mut value = 0_i64;
    let frame = |h: &mut UiHarness, value: &mut i64| {
        h.frame(|ui| {
            Slider::new(&mut *value, 0.0..=10.0)
                .size((Sizing::fixed(118.0), Sizing::fixed(18.0)))
                .id(id)
                .show(ui);
        });
    };
    frame(&mut h, &mut value);

    h.press_at(Vec2::new(89.0, 9.0));
    frame(&mut h, &mut value);
    assert_eq!(value, 8);

    h.drag_to(Vec2::new(34.0, 9.0));
    frame(&mut h, &mut value);
    assert_eq!(value, 3);
}

/// A focused slider walks by key: an arrow steps a hundredth of the range
/// toward `max` (right, up) or `min` (left, down), Shift ten of those, a
/// page key a tenth of the range, Home and End to the ends. A step snaps
/// the walk to it, a reversed range walks the same way along the track
/// (right is toward `max`, here down), and every key is a whole edit.
/// Unfocused or disabled, keys move nothing.
#[test]
fn a_focused_slider_walks_by_key() {
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::modifiers::Modifiers;

    /// Held modifiers, the key, the range, the step, and where 5.0 lands.
    type Case = (Modifiers, Key, (f64, f64), Option<f64>, f64);
    let id = WidgetId::from_hash("slider-keys");
    let cases: [Case; 11] = [
        (Modifiers::NONE, Key::ArrowRight, (0.0, 10.0), None, 5.1),
        (Modifiers::NONE, Key::ArrowUp, (0.0, 10.0), None, 5.1),
        (Modifiers::NONE, Key::ArrowLeft, (0.0, 10.0), None, 4.9),
        (Modifiers::SHIFT, Key::ArrowRight, (0.0, 10.0), None, 6.0),
        (Modifiers::NONE, Key::PageUp, (0.0, 10.0), None, 6.0),
        (Modifiers::NONE, Key::PageDown, (0.0, 10.0), None, 4.0),
        (Modifiers::NONE, Key::Home, (0.0, 10.0), None, 0.0),
        (Modifiers::NONE, Key::End, (0.0, 10.0), None, 10.0),
        (
            Modifiers::NONE,
            Key::ArrowRight,
            (0.0, 10.0),
            Some(0.5),
            5.5,
        ),
        (Modifiers::NONE, Key::ArrowRight, (10.0, 0.0), None, 4.9),
        (Modifiers::NONE, Key::End, (10.0, 0.0), None, 0.0),
    ];
    for (mods, key, (min, max), step, want) in cases {
        let mut h = UiHarness::new(UVec2::new(200, 40));
        let mut value = 5.0_f64;
        let frame = |h: &mut UiHarness, value: &mut f64| {
            h.frame_value(|ui| {
                let slider = Slider::new(&mut *value, min..=max).id(id);
                match step {
                    Some(s) => slider.step(s),
                    None => slider,
                }
                .show(ui)
                .edges()
            })
        };
        frame(&mut h, &mut value);
        h.set_focus(id);
        h.set_modifiers(mods);
        h.key(key);
        let edges = frame(&mut h, &mut value);
        assert_eq!(value, want, "{mods:?} {key:?} over {min}..={max}");
        assert!(edges.changed && edges.committed, "{key:?} is a whole edit");
    }

    let mut h = UiHarness::new(UVec2::new(200, 40));
    let mut value = 5.0_f64;
    for (focused, disabled) in [(false, false), (true, true)] {
        if focused {
            h.set_focus(id);
        }
        h.key(Key::ArrowRight);
        h.frame(|ui| {
            Slider::new(&mut value, 0.0..=10.0)
                .id(id)
                .disabled(disabled)
                .show(ui);
        });
        assert_eq!(value, 5.0, "focused {focused}, disabled {disabled}");
    }
}
