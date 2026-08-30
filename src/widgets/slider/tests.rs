use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::panel::Panel;
use crate::widgets::slider::{
    Slider, fraction_to_value, pointer_to_fraction, snap_to_step, value_to_fraction,
};
use glam::{UVec2, Vec2};

#[derive(Debug)]
struct Signals {
    changed: bool,
    committed: bool,
    /// Record passes that reported `committed` this frame. A frame
    /// records twice on action input, and an undo pusher applies once
    /// per pass — so a commit has to fire in exactly one of them.
    commits: u32,
}

/// One frame driven by a commit-deferring caller: the draft re-seeds
/// from `canonical` every record pass and is adopted only on
/// `committed`. Signals OR-accumulate across the frame's passes,
/// because a one-frame edge only shows in the first.
fn deferred_frame(h: &mut UiHarness, id: WidgetId, canonical: &mut f64) -> Signals {
    let mut s = Signals {
        changed: false,
        committed: false,
        commits: 0,
    };
    h.frame(|ui| {
        let mut draft = *canonical;
        let r = Slider::new(&mut draft, 0.0..=1.0)
            .size((Sizing::fixed(118.0), Sizing::fixed(18.0)))
            .id(id)
            .show(ui);
        s.changed |= r.changed;
        if r.committed {
            s.committed = true;
            s.commits += 1;
            *canonical = draft;
        }
    });
    s
}

/// The release frame re-writes the value, so a caller that re-seeds its
/// draft from a canonical copy every frame and adopts it only on
/// `committed` still observes the gesture's result. `Drag::Stopped` is
/// neither `pressed()` nor `dragging()`, so without naming it the
/// deferred caller would read its own seed back on the one frame it acts
/// on.
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
    assert!(s.changed && !s.committed, "press: live write, no commit");
    assert_eq!(canonical, 0.0, "deferred caller ignores mid-drag writes");

    h.drag_to(Vec2::new(89.0, 9.0));
    let s = deferred_frame(&mut h, id, &mut canonical);
    assert!(s.changed && !s.committed, "drag: live write, no commit");
    assert_eq!(canonical, 0.0);

    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical);
    assert!(s.committed, "release commits the gesture");
    assert_eq!(s.commits, 1, "one commit, one record pass");
    assert_eq!(canonical, 0.8, "the commit frame carries the final value");

    let s = deferred_frame(&mut h, id, &mut canonical);
    assert!(!s.changed && !s.committed, "no residual signals");
    assert_eq!(canonical, 0.8);
}

/// Explicit `.size(...)` wins over the widget's `Fill × knob_size`
/// default, and an untouched slider still gets that default
/// (400-wide FILL column → 400 × knob_size 18).
#[test]
fn explicit_size_overrides_fill_default() {
    let mut h = UiHarness::new(UVec2::new(400, 300));
    let mut v = 0.5_f64;
    let (mut sized, mut hug, mut default) = (None, None, None);
    h.frame(|ui| {
        let col = Panel::vstack().auto_id().size((Sizing::FILL, Sizing::FILL));
        col.show(ui, |ui| {
            sized = Some(
                Slider::new(&mut v, 0.0..=1.0)
                    .size((Sizing::fixed(120.0), Sizing::fixed(30.0)))
                    .show(ui)
                    .response
                    .node(),
            );
            hug = Some(
                Slider::new(&mut v, 0.0..=1.0)
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui)
                    .response
                    .node(),
            );
            default = Some(Slider::new(&mut v, 0.0..=1.0).show(ui).response.node());
        });
    });
    let rects = &h.ui.layout(Layer::Main).rect;
    let s = rects[sized.unwrap().idx()];
    assert_eq!((s.size.w, s.size.h), (120.0, 30.0), "explicit size");
    let h = rects[hug.unwrap().idx()];
    assert_eq!((h.size.w, h.size.h), (18.0, 18.0), "explicit hug");
    let d = rects[default.unwrap().idx()];
    assert_eq!((d.size.w, d.size.h), (400.0, 18.0), "untouched default");
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
    ];
    for (v, min, max, want) in cases {
        let got = value_to_fraction(v, min, max);
        assert!(
            (got - want).abs() < 1e-6,
            "v2f({v},{min},{max})={got} want {want}"
        );
    }
    // A NaN anywhere in the triple names no share, and the low end is
    // what this widget reads that as — the same answer
    // `pointer_to_fraction` gives a track with no travel.
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
        assert!((back - v).abs() < 1e-5, "roundtrip {v} -> {f} -> {back}");
    }
    assert!((fraction_to_value(0.25, 10.0, 20.0) - 12.5).abs() < 1e-6);
    // Out-of-range fraction clamps before mapping.
    assert!((fraction_to_value(1.5, 0.0, 100.0) - 100.0).abs() < 1e-6);
}

#[test]
fn pointer_to_fraction_uses_knob_inset_travel() {
    let track_w = 120.0;
    let knob = 20.0; // travel = 100, offset knob/2 = 10
    assert!((pointer_to_fraction(10.0, track_w, knob) - 0.0).abs() < 1e-6);
    assert!((pointer_to_fraction(110.0, track_w, knob) - 1.0).abs() < 1e-6);
    assert!((pointer_to_fraction(60.0, track_w, knob) - 0.5).abs() < 1e-6);
    // Past the ends clamps.
    assert!((pointer_to_fraction(0.0, track_w, knob) - 0.0).abs() < 1e-6);
    assert!((pointer_to_fraction(200.0, track_w, knob) - 1.0).abs() < 1e-6);
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

            let response = h.ui.response_for(id);
            let layout = response.layout_rect.expect("slider arranged");
            let pointer = response
                .transform
                .apply_point(layout.min + Vec2::new(local_x, 9.0));
            h.press_at(pointer);
            h.frame(|ui| build(ui, &mut value));

            assert!(
                (value - expected).abs() < 1e-6,
                "logical x={local_x} at {scale}× produced {value}, expected {expected}",
            );
        }
    }
}

#[test]
fn snap_to_step_rounds_to_grid() {
    assert!((snap_to_step(53.0, 0.0, Some(10.0)) - 50.0).abs() < 1e-6);
    assert!((snap_to_step(57.0, 0.0, Some(10.0)) - 60.0).abs() < 1e-6);
    assert!((snap_to_step(12.0, 0.0, Some(5.0)) - 10.0).abs() < 1e-6);
    assert!((snap_to_step(13.0, 0.0, Some(5.0)) - 15.0).abs() < 1e-6);
    // Off-anchor grid: steps of 0.5 from min=1.0.
    assert!((snap_to_step(2.2, 1.0, Some(0.5)) - 2.0).abs() < 1e-6);
    // A slider with no step passes the value through.
    assert!((snap_to_step(53.0, 0.0, None) - 53.0).abs() < 1e-6);
}

/// `None` is the only "off": the builder refuses a step that would be a
/// second spelling of it.
#[test]
fn step_rejects_a_value_that_cannot_snap() {
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            std::panic::catch_unwind(move || {
                let mut v = 0.5_f64;
                Slider::new(&mut v, 0.0..=1.0).step(bad);
            })
            .is_err(),
            "step({bad}) must panic",
        );
    }
}

/// A track with no travel left reports the low end rather than dividing
/// by a floored span.
#[test]
fn pointer_to_fraction_reports_zero_for_a_knob_wider_than_its_rail() {
    assert_eq!(pointer_to_fraction(15.0, 20.0, 20.0), 0.0);
    assert_eq!(pointer_to_fraction(15.0, 10.0, 20.0), 0.0);
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
