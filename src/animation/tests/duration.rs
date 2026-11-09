//! Fixed-duration tweens: snapping, floors, and finite settling.

use crate::animation::animation_map_typed::AnimationMapTyped;
use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_spec::AnimationSpec;
use crate::animation::easing::Easing;
use crate::animation::tests::support::{self, AnimationUi, SLOT};
use crate::common::time::MAX_ANIMATION_DT;
use crate::internals::panic_probe;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::math::domain;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::math::libm_ext::Libm;
use crate::primitives::paint::color::RgbaF32;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use glam::Vec2;
use std::time::Duration;

/// A zero-second `Duration` spec behaves like `SNAP` through `Ui::animate`: it snaps, drops any in-flight row.
#[test]
fn instant_duration_is_noop_and_drops_row() {
    let instant = AnimationSpec::duration(Duration::ZERO, Easing::Linear);
    let AnimationUi { mut h, id } = support::setup_animation_ui("anim-instant");

    let passes = h.at(Duration::from_millis(0)).frame_passes(|ui| {
        let v = ui
            .animate(AnimationRequest::new(id, SLOT, 1.0_f32), instant)
            .value;
        Block::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
        v
    });
    assert_eq!(*passes.a(), 1.0);
    assert!(!passes.report().repaint_requested);
    assert_eq!(h.animation_row_count::<f32>(), 0);

    let _ = h.at(Duration::from_millis(0)).frame(|ui| {
        let _ = ui.animate(
            AnimationRequest::new(id, SLOT, 0.0_f32),
            AnimationSpec::FAST,
        );
        Block::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
    });
    let _ = h.at(Duration::from_millis(50)).frame(|ui| {
        let _ = ui.animate(
            AnimationRequest::new(id, SLOT, 1.0_f32),
            AnimationSpec::FAST,
        );
        Block::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
    });
    assert_eq!(h.animation_row_count::<f32>(), 1);

    let v = h.at(Duration::from_millis(60)).frame_value(|ui| {
        let v = ui
            .animate(AnimationRequest::new(id, SLOT, 1.0_f32), instant)
            .value;
        Block::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
        v
    });
    assert_eq!(v, 1.0);
    assert_eq!(
        h.animation_row_count::<f32>(),
        0,
        "instant must drop the stale row inserted by FAST",
    );

    let v = h.at(Duration::from_millis(70)).frame_value(|ui| {
        let v = ui
            .animate(
                AnimationRequest::new(id, SLOT, 5.0_f32),
                AnimationSpec::FAST,
            )
            .value;
        Block::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
        v
    });
    assert_eq!(v, 5.0, "post-instant first-touch snaps to new target");
}

/// Sub-perceptual drift snaps instead of animating, using the type's own settle tolerance: `1e-5` on an
/// `f32` (1e-4) snaps; `2e-4` snaps on a color's red at mid grey (4.63e-5 in motion space, under `2^-14`) but
/// animates on an `f32`.
#[test]
fn target_below_snap_floor_snaps_without_animating() {
    let duration = AnimationSpec::duration(Duration::from_secs(1), Easing::Linear);
    let tiny = 1.0e-5;
    let cases: &[(&str, AnimationSpec)] =
        &[("duration", duration), ("spring", AnimationSpec::SPRING)];
    for (label, spec) in cases {
        let mut map = AnimationMapTyped::<f32>::default();
        let id = support::widget_id("a");
        let _ = map.step(AnimationRequest::new(id, SLOT, 0.0), *spec, 0.016);
        let r = map.step(AnimationRequest::new(id, SLOT, tiny), *spec, 0.016);
        assert_eq!(
            r.value, tiny,
            "case {label}: snap-if-close must reach new target exactly",
        );
        assert!(
            r.settled,
            "case {label}: sub-eps drift must report settled (no repaint)",
        );
        assert!(
            map.rows[&(id, SLOT)].motion.is_none(),
            "case {label}: a change that snaps starts no run",
        );

        let mut scalars = AnimationMapTyped::<f32>::default();
        let _ = scalars.step(AnimationRequest::new(id, SLOT, 0.5), *spec, 0.016);
        let r = scalars.step(AnimationRequest::new(id, SLOT, 0.5 + 2e-4), *spec, 0.016);
        assert!(!r.settled, "case {label}: 2e-4 on an f32 animates");
        assert!(
            scalars.rows[&(id, SLOT)].motion.is_some(),
            "case {label}: a change that animates starts a run",
        );

        let mut colors = AnimationMapTyped::<RgbaF32>::default();
        let (gray, nudged) = (
            RgbaF32::new(0.5, 0.5, 0.5, 1.0),
            RgbaF32::new(0.5 + 2e-4, 0.5, 0.5, 1.0),
        );
        let _ = colors.step(AnimationRequest::new(id, SLOT, gray), *spec, 0.016);
        let r = colors.step(AnimationRequest::new(id, SLOT, nudged), *spec, 0.016);
        assert_eq!(r.value, nudged, "case {label}: a color lands on target");
        assert!(r.settled, "case {label}: 2e-4 on a color snaps");
    }
}

/// One tolerance for both motions: 5e-4 exceeds an `f32`'s 1e-4, so it animates under a duration and a
/// spring; the retarget frame shows the start value, the next moves toward the target. The spring case is
/// a dark-theme hover (`#121212 → #1c1c1c`, ~0.0056 linear per channel).
#[test]
fn one_floor_animates_a_small_change_under_either_motion() {
    let delta = 5.0e-4_f32;
    let duration = AnimationSpec::duration(Duration::from_secs(1), Easing::Linear);
    let t = f64::from(0.016f32);
    let spring_travel = 1.0 - (-13.0 * t).libm_exp() * (t.libm_cos() + 13.0 * t.libm_sin());
    for (label, spec) in [("spring", AnimationSpec::SPRING), ("duration", duration)] {
        let mut map = AnimationMapTyped::<f32>::default();
        let id = support::widget_id(label);
        let _ = map.step(AnimationRequest::new(id, SLOT, 0.0), spec, 0.016);
        let start = map.step(AnimationRequest::new(id, SLOT, delta), spec, 0.016);
        assert_eq!(
            start.value, 0.0,
            "{label}: the change's frame shows the start"
        );
        assert!(!start.settled, "{label}: a change above the floor animates");
        let moving = map.step(AnimationRequest::new(id, SLOT, delta), spec, 0.016);
        // Linear: 0.016 of the way over 1 s. Spring from rest: `delta·(1 − e^(-13t)(cos t + 13 sin t))` at t = 0.016.
        let expected = match label {
            "duration" => f64::from(delta * 0.016),
            _ => f64::from(delta) * spring_travel,
        };
        domain::internals::assert_close(
            moving.value,
            expected,
            1e-10,
            "the f32 step's coefficients are each within an ulp of the \
             closed form's, ~1e-7 of delta",
        );
    }

    let mut colors = AnimationMapTyped::<RgbaF32>::default();
    let id = support::widget_id("hover");
    let (rest, hover) = (RgbaF32::hex(0x121212), RgbaF32::hex(0x1c1c1c));
    let _ = colors.step(
        AnimationRequest::new(id, SLOT, rest),
        AnimationSpec::SPRING,
        0.016,
    );
    let _ = colors.step(
        AnimationRequest::new(id, SLOT, hover),
        AnimationSpec::SPRING,
        0.016,
    );
    let moving = colors.step(
        AnimationRequest::new(id, SLOT, hover),
        AnimationSpec::SPRING,
        0.016,
    );
    assert!(!moving.settled, "the hover fades over several frames");
    // A neutral's Oklab lightness is the cube root of its linear value, so the spring travels its
    // share in lightness and the channel is that lightness cubed.
    let lightness = |c: RgbaF32| f64::from(c.r).libm_cbrt();
    let expected =
        (lightness(rest) + (lightness(hover) - lightness(rest)) * spring_travel).libm_powi(3);
    domain::internals::assert_close(
        moving.value.r,
        expected,
        1e-8,
        "the first step travels the closed form's share, to the f32 rounding of an Oklab trip \
         of a value near 0.007",
    );
}

#[test]
fn first_touch_returns_target_and_settled() {
    for (label, spec) in [
        ("duration", AnimationSpec::FAST),
        ("spring", AnimationSpec::SPRING),
    ] {
        let mut map = AnimationMapTyped::<f32>::default();
        let id = support::widget_id(label);
        let r = map.step(AnimationRequest::new(id, SLOT, 1.0), spec, 0.016);
        assert_eq!(r.value, 1.0, "{label}: first touch must snap");
        assert!(r.settled, "{label}: first touch must report settled");
        assert!(
            map.rows[&(id, SLOT)].motion.is_none(),
            "{label}: a first touch runs nothing"
        );
    }
}

#[test]
fn duration_settles_in_finite_steps() {
    let mut map = AnimationMapTyped::<f32>::default();
    let id = support::widget_id("a");
    let spec = support::linear_100ms();
    let _ = map.step(AnimationRequest::new(id, SLOT, 0.0), spec, 0.016);
    let r = map.step(AnimationRequest::new(id, SLOT, 1.0), spec, 0.016);
    assert_eq!(r.value, 0.0);
    let r = map.step(AnimationRequest::new(id, SLOT, 1.0), spec, 0.05);
    assert_eq!(r.value, 0.05 / 0.1, "linear: elapsed over secs");
    assert!(!r.settled, "halfway is not settled");
    assert!(map.rows[&(id, SLOT)].motion.is_some(), "the run holds it");
    let r = map.step(AnimationRequest::new(id, SLOT, 1.0), spec, 0.05);
    assert_eq!(r.value, 1.0, "must snap to target on settle");
    assert!(r.settled, "100ms total elapsed must settle");
    assert!(map.rows[&(id, SLOT)].motion.is_none(), "rest frees it");

    let mut boundary_map = AnimationMapTyped::<f32>::default();
    let boundary_id = support::widget_id("maximum-duration");
    let boundary = AnimationSpec::duration(Duration::from_secs(60), Easing::Linear);
    let _ = boundary_map.step(AnimationRequest::new(boundary_id, SLOT, 0.0), boundary, 0.0);
    // The change's frame spends nothing, then 600 steps of 0.1 s run the 60 s; one more absorbs f32 rounding.
    let mut settled = None;
    for step in 0..=601 {
        let result = boundary_map.step(
            AnimationRequest::new(boundary_id, SLOT, 1.0),
            boundary,
            MAX_ANIMATION_DT,
        );
        assert!(result.value.is_finite());
        if result.settled {
            assert_eq!(result.value, 1.0);
            settled = Some(step);
            break;
        }
    }
    assert_eq!(settled, Some(601));
}

#[test]
fn dt_zero_does_not_advance_duration() {
    let mut map = AnimationMapTyped::<f32>::default();
    let id = support::widget_id("a");
    let spec = support::linear_100ms();
    let _ = map.step(AnimationRequest::new(id, SLOT, 0.0), spec, 0.0);
    let _ = map.step(AnimationRequest::new(id, SLOT, 1.0), spec, 0.0);
    let r = map.step(AnimationRequest::new(id, SLOT, 1.0), spec, 0.0);
    assert_eq!(r.value, 0.0, "dt=0 must not advance toward target");
    assert!(!r.settled, "still in flight");
}

#[test]
fn vec2_duration_lerps_componentwise() {
    let mut map = AnimationMapTyped::<Vec2>::default();
    let id = support::widget_id("a");
    let spec = support::linear_100ms();
    let _ = map.step(AnimationRequest::new(id, SLOT, Vec2::ZERO), spec, 0.0);
    let _ = map.step(
        AnimationRequest::new(id, SLOT, Vec2::new(10.0, 20.0)),
        spec,
        0.0,
    );
    let r = map.step(
        AnimationRequest::new(id, SLOT, Vec2::new(10.0, 20.0)),
        spec,
        0.05,
    );
    let progress = 0.05 / 0.1;
    assert_eq!(
        r.value,
        Vec2::new(10.0 * progress, 20.0 * progress),
        "each component lerps by the same linear progress",
    );
}

/// `OutBack` overshoots its target before settling, however small the change; a snap floor checked
/// mid-curve would end the motion at the crossing frame.
#[test]
fn out_back_reaches_its_overshoot_on_a_small_change() {
    let mut map = AnimationMapTyped::<f32>::default();
    let id = support::widget_id("out-back");
    let spec = AnimationSpec::duration(Duration::from_millis(200), Easing::OutBack);
    let _ = map.step(AnimationRequest::new(id, SLOT, 0.0), spec, 0.016);
    let mut peak = 0.0_f32;
    for _ in 0..30 {
        let r = map.step(AnimationRequest::new(id, SLOT, 0.01), spec, 1.0 / 60.0);
        peak = peak.max(r.value);
        if r.settled {
            assert_eq!(r.value, 0.01);
            break;
        }
    }
    // Frame `k` after the retarget has eased `k/60` s of 0.2 s. `OutBack` peaks at progress
    // `1 - 2·C1/(3·C3)` ≈ 0.580, so frame 7 (7/12 ≈ 0.583) is the highest, at 0.01 · 1.09999.
    let elapsed = (0..7).fold(0.0_f32, |elapsed, _| elapsed + 1.0 / 60.0);
    assert_eq!(peak, 0.01 * Easing::OutBack.apply(elapsed / 0.2));
}

/// A non-finite target is a caller logic error: NaN != NaN would retarget every frame forever, so
/// a release build refuses it too.
#[test]
fn a_non_finite_target_is_refused() {
    for target in [f32::NAN, f32::INFINITY] {
        let mut map = AnimationMapTyped::<f32>::default();
        let id = support::widget_id("nan");
        let _ = map.step(
            AnimationRequest::new(id, SLOT, 0.0),
            AnimationSpec::FAST,
            0.016,
        );
        panic_probe::assert_panics_with("is not finite", || {
            map.step(
                AnimationRequest::new(id, SLOT, target),
                AnimationSpec::FAST,
                0.016,
            )
        });
    }
}

/// A run keeps the spec it started under: an OutCubic run of 120 ms at 60 ms shows 87.5 % of the
/// way from 0 to 1, and frames under 200 ms or Linear keep that value and land on 1 by 130 ms,
/// as the 120 ms run does. A finished run stays at rest under any spec. The next change takes the
/// new spec: back from 1 to 0, 60 ms in shows `1 − OutCubic(0.3) = 0.7³ = 0.343` under 200 ms,
/// and `1 − 0.5` under Linear 120 ms.
#[test]
fn a_new_length_or_ease_waits_for_the_next_change() {
    let ms = |ms: u64| Duration::from_millis(ms);
    let fast = AnimationSpec::duration(ms(120), Easing::OutCubic);
    for (switched, back) in [
        (AnimationSpec::duration(ms(200), Easing::OutCubic), 0.343),
        (AnimationSpec::duration(ms(120), Easing::Linear), 0.5),
    ] {
        let mut map = AnimationMapTyped::<f32>::default();
        let id = support::widget_id("switch");
        let mut step = |target: f32, spec: AnimationSpec, dt: f32| {
            map.step(AnimationRequest::new(id, SLOT, target), spec, dt)
        };
        let _ = step(0.0, fast, 0.0);
        let _ = step(1.0, fast, 0.0);
        let at = step(1.0, fast, 0.06).value;
        assert!((at - 0.875).abs() < 1e-6, "{at}");
        let held = step(1.0, switched, 0.0);
        assert_eq!((held.value, held.settled), (at, false), "{switched:?}");
        let done = step(1.0, switched, 0.07);
        assert_eq!((done.value, done.settled), (1.0, true), "{switched:?}");
        let rest = step(1.0, fast, 0.01);
        assert_eq!((rest.value, rest.settled), (1.0, true), "{switched:?}");

        assert_eq!(
            step(0.0, switched, 0.0).value,
            1.0,
            "{switched:?}: from rest"
        );
        let on = step(0.0, switched, 0.06).value;
        // Six decimal places: `back` is the exact decimal, `on` its f32 rounding through the ease.
        assert!((on - back).abs() < 1e-6, "{switched:?}: {on}");
    }
}

/// Each easing's paint curve is its tween curve: `1 − 0.75³ = 0.578125` for `OutCubic` at a
/// quarter, `4 · 0.25³ = 0.0625` for `InOutCubic`, `1 − 0.75⁴ = 0.68359375` for `OutQuart`, and
/// `OutBack` past 1 at three quarters, `1 + 2.70158 · (−0.25)³ + 1.70158 · 0.25² ≈ 1.0641`.
#[test]
fn an_easing_curve_is_its_tween() {
    let all = [
        Easing::Linear,
        Easing::OutCubic,
        Easing::InOutCubic,
        Easing::OutQuart,
        Easing::OutBack,
    ];
    for easing in all {
        for i in 0..=16 {
            let t = i as f32 / 16.0;
            assert_eq!(easing.curve()(t), easing.apply(t), "{easing:?} at {t}");
        }
    }
    let quarter = all.map(|easing| easing.curve()(0.25));
    assert_eq!(quarter[..4], [0.25, 0.578125, 0.0625, 0.68359375]);
    assert!(
        Easing::OutBack.curve()(0.75) > 1.0,
        "the overshoot survives"
    );
}

/// An overshooting curve toward smaller lengths paints none below zero: each length a chrome
/// holds is coerced at its floor, so the value a widget validates holds. `OutBack` from a chrome
/// of radius 8, border 4, shadow blur 6 and frost 10 to one of all zeros peaks at frame 7, at
/// `1 − OutBack(7/12) ≈ −0.09999` of the way back: −0.8, −0.4, −0.6 and −1.0 uncoerced, each 0.
#[test]
fn an_overshoot_below_a_floor_paints_the_floor() {
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::paint::border::Border;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::material::Material;
    use crate::primitives::paint::shadow::Shadow;

    let ink = RgbaF32::srgb(0.2, 0.4, 0.8);
    let chrome = |radius: f32, width: f32, blur: f32, frost: f32| {
        Chrome::rounded(ink, radius)
            .with_border(Border::new(ink, width))
            .with_shadow(Shadow::drop(ink, Vec2::new(0.0, 2.0), blur))
            .with_material(Material::frosted(frost))
    };
    let mut map = AnimationMapTyped::<Chrome>::default();
    let id = support::widget_id("overshoot");
    let spec = AnimationSpec::duration(Duration::from_millis(200), Easing::OutBack);
    let _ = map.step(
        AnimationRequest::new(id, SLOT, chrome(8.0, 4.0, 6.0, 10.0)),
        spec,
        0.016,
    );
    let target = chrome(0.0, 0.0, 0.0, 0.0);
    // A blend past its end coerces alike, returned and in place: at 1.1, each length lands a
    // tenth past zero, and is zero.
    let from = chrome(8.0, 4.0, 6.0, 10.0);
    let past = Chrome::lerp(&from, &target, 1.1);
    let mut in_place = Chrome::NONE;
    Chrome::lerp_into(&from, &target, 1.1, &mut in_place);
    assert_eq!(in_place, past);
    past.validate();
    assert_eq!(past.paint.border.width, 0.0);
    for frame in 1..=12 {
        let r = map.step(
            AnimationRequest::new(id, SLOT, target.clone()),
            spec,
            1.0 / 60.0,
        );
        r.value.validate();
        if frame == 7 {
            assert_eq!(r.value.corners, Corners::ZERO);
            assert_eq!(r.value.paint.border.width, 0.0);
            assert_eq!(r.value.paint.shadow.blur, 0.0);
            assert_eq!(r.value.paint.material.map(|m| m.blur), Some(0.0));
        }
    }
}
