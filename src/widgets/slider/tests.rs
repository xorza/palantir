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

/// One frame for a commit-deferring caller: the draft re-seeds from `canonical` and is adopted only on `committed`.
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

/// The release frame re-writes the value, so a deferred caller sees the result.
/// Geometry: 118 wide, knob 18, travel 100 px from x = 9; x = 59 is 0.5, x = 89 is 0.8.
#[test]
fn release_rewrites_the_value_once_for_a_deferred_caller() {
    let id = WidgetId::from_hash("slider-deferred-commit");
    let mut h = UiHarness::new(UVec2::new(118, 18));
    let mut canonical = 0.0_f64;

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

/// A press and release on the track is a whole edit: it writes a value, latches no drag, and commits once.
#[test]
fn a_click_on_the_track_commits_the_value_it_wrote() {
    let id = WidgetId::from_hash("slider-click-commit");
    let mut h = UiHarness::new(UVec2::new(118, 18));
    let mut canonical = 0.0_f64;

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

/// Explicit `.size(...)` wins over the `Fill × knob_size` default; hugged, it is the knob alone.
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

/// An endpoint collapses one track segment to zero extent, and a NaN value lays out as the low end.
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
        (5e-6, 0.0, 1e-5, 0.5),
        (5e99, 0.0, 1e100, 0.5),
        (2.5e-7, 0.0, 1e-5, 0.025),
        (1e300, 0.0, 1.0, 1.0),
        (-1e300, 0.0, 1.0, 0.0),
        (1.0, 0.0, 1e-300, 1.0),
        (50.0, 100.0, 0.0, 0.5),
        (100.0, 100.0, 0.0, 0.0),
        (0.0, 100.0, 0.0, 1.0),
    ];
    for (v, min, max, want) in cases {
        let got = value_to_fraction(v, min, max);
        assert_eq!(got, want, "v2f({v},{min},{max})={got} want {want}");
    }
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
    for &v in &[10.0_f64, 12.5, 15.0, 17.5, 20.0] {
        let f = value_to_fraction(v, 10.0, 20.0);
        let back = fraction_to_value(f, 10.0, 20.0);
        assert_eq!(back, v, "roundtrip {v} -> {f} -> {back}");
    }
    assert_eq!(fraction_to_value(0.25, 10.0, 20.0), 12.5);
    let f = value_to_fraction(75.0, 100.0, 0.0);
    assert_eq!(f, 0.25, "reversed fraction {f}");
    assert_eq!(fraction_to_value(f, 100.0, 0.0), 75.0);
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
    assert_eq!(snap_to_step(2.2, 1.0, Some(0.5)), 2.0);
    assert_eq!(snap_to_step(53.0, 0.0, None), 53.0);
}

/// `None` is the only "off"; a step that cannot snap is refused.
#[test]
fn step_rejects_a_value_that_cannot_snap() {
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        panic_probe::assert_panics_with("a positive value must be finite and above zero", || {
            let mut v = 0.5_f64;
            let _ = Slider::new(&mut v, 0.0..=1.0).step(bad);
        });
    }
}

/// An infinite or NaN end is refused at build: a click would store `+inf` or `NaN`.
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

/// A `DragNum` integer target lands on whole values: on `0..=10`, x = 34 is 2.5, rounding away from zero to 3.
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

/// A focused slider walks by key: arrows a hundredth of the range, Shift ten
/// of those, page keys a tenth, Home/End to the ends. A step snaps; every key
/// is a whole edit; unfocused or disabled, keys move nothing.
#[test]
fn a_focused_slider_walks_by_key() {
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::modifiers::Modifiers;

    /// Modifiers, key, range, step, and where 5.0 lands.
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
