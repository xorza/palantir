//! The spring transition: closed-form agreement, independence from frame
//! partitioning, and row settling.

use crate::animation::animation_spec::AnimMotion;
use crate::animation::easing::Easing;
use crate::animation::tests::support::{
    SLOT, closed_form_settle_step, duration_motion, spring_velocity, wid,
};
use crate::animation::*;
use crate::common::time::ANIM_SUBSTEP_DT;
use crate::primitives::math::domain::internals::assert_close;
use crate::primitives::paint::color::RgbaF32;

/// Accepted springs stay finite and settle on the step the closed form names. Step
/// 0 is the change's frame.
#[test]
fn validated_springs_remain_finite_and_settle() {
    let cases = [
        ("minimum-decay", AnimationSpec::spring(1.0, 2.0), 488),
        ("default", AnimationSpec::SPRING, 39),
        (
            "stiff, critically damped",
            AnimationSpec::spring(1_000_000.0, 2_000.0),
            2,
        ),
    ];
    let dts = [0.1, 1.0 / 60.0, 0.0042, 0.033];

    for (label, spec, expected) in cases {
        let AnimMotion::Spring { stiffness, damping } = spec.motion else {
            unreachable!("every case is a spring");
        };
        assert_eq!(
            closed_form_settle_step(f64::from(stiffness), f64::from(damping), 500.0, 1e-4, |n| {
                dts[n as usize % dts.len()]
            }),
            expected,
            "{label}: closed form",
        );
        let mut map = AnimMapTyped::<f32>::default();
        let id = wid(label);
        let _ = map.step(id, SLOT, 400.0, spec, dts[0]);
        let mut settled_at = None;
        for i in 0..4_000_u32 {
            let result = map.step(id, SLOT, -100.0, spec, dts[i as usize % dts.len()]);
            let row = &map.rows[&(id, SLOT)];
            let velocity = *spring_velocity(row);
            assert!(
                result.current.is_finite() && velocity.is_finite(),
                "{label} became non-finite at step {i}: {row:?}",
            );
            if result.settled {
                assert_eq!(result.current, -100.0, "{label} did not snap to target");
                assert_eq!(velocity, 0.0, "{label} retained settled velocity");
                settled_at = Some(i);
                break;
            }
        }
        assert_eq!(settled_at, Some(expected), "{label}");
    }
}

/// The three damping regimes against an independent solution of `x'' + c·x' + k·x =
/// 0`, plus a stiff overdamped corner where a textbook `e^(-h·t)·cosh(ψ·t)`
/// overflows `f64`. Released from rest at `x₀ = 1`; expected values are
/// hand-evaluated per regime.
#[test]
fn closed_form_matches_the_analytic_solution_in_every_damping_regime() {
    let cases = [
        ("underdamped", 5.0, 2.0, 0.976_682_66, -0.449_408_62),
        ("critically damped", 100.0, 20.0, 0.735_758_9, -3.678_794_4),
        ("overdamped", 4.0, 5.0, 0.983_009_9, -0.312_689_84),
        (
            "stiff overdamped",
            1.0e6,
            20_000.0,
            0.006_670_589,
            -0.334_367_46,
        ),
    ];
    for (label, stiffness, damping, expect_pos, expect_vel) in cases {
        assert!(
            spring::params_are_valid(stiffness, damping),
            "{label}: fixture must be an accepted spring",
        );
        let step = spring::step(stiffness, damping, 1.0_f32, 0.0, 0.1);
        assert!(!step.settled, "{label}: a unit displacement is not settled");
        assert_close(
            step.offset,
            expect_pos,
            1e-6,
            "f32 transition coefficients against the analytic position",
        );
        assert_close(
            step.velocity,
            expect_vel,
            1e-5,
            "f32 transition coefficients against the analytic velocity, \
             which the stiffness scales",
        );
    }
}

/// For a held target, travel over an interval is independent of how frames
/// partition it; a substepped Euler integrator fails this.
#[test]
fn travel_does_not_depend_on_how_the_frames_partition_it() {
    let (stiffness, damping) = (170.0, 26.0);
    let travel = |steps: u32| {
        let dt = 0.1 / steps as f32;
        let mut current = 300.0_f32;
        let mut velocity = 0.0_f32;
        for _ in 0..steps {
            let step = spring::step(stiffness, damping, current, velocity, dt);
            assert!(
                !step.settled,
                "fixture must stay in flight for the whole 0.1s"
            );
            current = step.offset;
            velocity = step.velocity;
        }
        current
    };
    let once = travel(1);
    // `(170, 26)` has ψ = 1: 300 px becomes ≈ 187 px after 0.1 s, still in flight.
    assert!((185.0..190.0).contains(&once), "fixture sanity: got {once}");
    for steps in [2, 10, 100] {
        let split = travel(steps);
        assert_close(
            split,
            once,
            0.01,
            "each step rounds its f32 coefficients, so the products drift \
             by a few ulps per step at 187 px",
        );
    }
}

/// Critical damping is the `ψ = 0` point of the exponential branch, so the
/// transition is continuous across `damping = 2√stiffness` with no epsilon-wide
/// seam.
#[test]
fn the_critically_damped_boundary_has_no_seam() {
    let stiffness = 100.0_f32;
    let at = |damping: f32| spring::step(stiffness, damping, 1.0_f32, 0.0, 0.1).offset;
    let critical = at(20.0);
    for offset in [1e-3, 1e-4, 1e-5] {
        let under = at(20.0 - offset);
        let over = at(20.0 + offset);
        // Near critical, x(0.1) moves about 0.0061 per unit of damping; a seam
        // would hold its width as the offset shrinks.
        let bound = 0.007 * f64::from(offset) + 6e-8;
        let why = "the damping offset times the slope, plus an ulp";
        assert_close(under, critical, bound, why);
        assert_close(over, critical, bound, why);
    }
}

#[test]
fn spring_parameters_change_trajectory() {
    let mut default_map = AnimMapTyped::<f32>::default();
    let mut custom_map = AnimMapTyped::<f32>::default();
    let id = wid("spring-parameters");
    let custom = AnimationSpec::spring(100.0, 15.0);
    let _ = default_map.step(id, SLOT, 0.0, AnimationSpec::SPRING, 0.016);
    let _ = custom_map.step(id, SLOT, 0.0, custom, 0.016);
    let _ = default_map.step(id, SLOT, 1.0, AnimationSpec::SPRING, 0.016);
    let _ = custom_map.step(id, SLOT, 1.0, custom, 0.016);
    let default = default_map
        .step(id, SLOT, 1.0, AnimationSpec::SPRING, 0.016)
        .current;
    let custom = custom_map.step(id, SLOT, 1.0, custom, 0.016).current;
    assert_ne!(default, custom);
}

/// `MAX_ANIM_DT` must not throw the value past its endpoints: one Euler step at `dt
/// = 0.1` with `(170, 26)` once overshot negative, tripping `Sizing::fixed`.
#[test]
fn spring_step_at_max_dt_stays_bounded() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let _ = map.step(id, SLOT, 400.0, AnimationSpec::SPRING, 0.016);
    let r = map.step(id, SLOT, 80.0, AnimationSpec::SPRING, 0.1);
    assert_eq!(r.current, 400.0);
    let r = map.step(id, SLOT, 80.0, AnimationSpec::SPRING, 0.1);
    // Released 320 px out, `(170, 26)` has h = 13, ω = 1: `80 + 320·e^(-1.3)(cos
    // 0.1 + 13 sin 0.1)` ≈ 279.96.
    let t = f64::from(0.1f32);
    let expected = 80.0 + 320.0 * (-13.0 * t).exp() * (t.cos() + 13.0 * t.sin());
    assert_close(r.current, expected, 1e-4, "a few f32 ulps at 280");
}

/// A frame may run `build` twice; the guard on `render_frame_id` advances the
/// integrator once. A pass B retarget still takes effect without another `dt`.
#[test]
fn second_tick_in_same_frame_does_not_double_advance() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let frame = 42;

    let _ = map.tick(id, SLOT, 0.0, AnimationSpec::FAST, 0.016, frame - 2);

    let from_rest_a = map.tick(id, SLOT, 1.0, AnimationSpec::FAST, 0.016, frame - 1);
    let from_rest_b = map.tick(id, SLOT, 1.0, AnimationSpec::FAST, 0.016, frame - 1);
    assert_eq!((from_rest_a.current, from_rest_b.current), (0.0, 0.0));

    let pass_a = map.tick(id, SLOT, 1.0, AnimationSpec::FAST, 0.016, frame);
    let eased = Easing::OutCubic.apply(0.016 / 0.12);
    assert_eq!(pass_a.current, eased, "FAST: 16 ms of 120 ms, out-cubic");
    let pass_a_current = pass_a.current;
    let pass_a_elapsed = duration_motion(&map.rows[&(id, SLOT)]).elapsed;

    let pass_b = map.tick(id, SLOT, 1.0, AnimationSpec::FAST, 0.016, frame);
    assert_eq!(
        pass_b.current, pass_a_current,
        "pass B with same render_frame_id must not advance current",
    );
    assert_eq!(
        duration_motion(&map.rows[&(id, SLOT)]).elapsed,
        pass_a_elapsed,
        "pass B with same render_frame_id must not advance elapsed",
    );

    let pass_b_retarget = map.tick(id, SLOT, 5.0, AnimationSpec::FAST, 0.016, frame);
    assert_eq!(
        pass_b_retarget.current, pass_a_current,
        "retargeting in pass B updates segment but doesn't re-step",
    );
    assert_eq!(map.rows[&(id, SLOT)].target, 5.0);
    assert_eq!(
        *duration_motion(&map.rows[&(id, SLOT)]).segment_start,
        pass_a_current
    );

    let next = map.tick(id, SLOT, 5.0, AnimationSpec::FAST, 0.016, frame + 1);
    assert_eq!(
        next.current,
        pass_a_current + (5.0 - pass_a_current) * eased,
        "next frame eases 16 ms toward 5.0 from pass A's current",
    );
}

/// A retarget along the motion keeps velocity (fling-through); against it, velocity
/// is zeroed so the new segment cannot swing far past the target.
#[test]
fn spring_retarget_zeroes_opposing_velocity_only() {
    let mut map = AnimMapTyped::<f32>::default();

    let id_aligned = wid("aligned");
    let _ = map.step(id_aligned, SLOT, 0.0, AnimationSpec::SPRING, 0.016);
    for _ in 0..3 {
        let _ = map.step(id_aligned, SLOT, 1.0, AnimationSpec::SPRING, 0.016);
    }
    let v_before = *spring_velocity(&map.rows[&(id_aligned, SLOT)]);
    assert!(v_before > 0.0, "precondition: moving toward 1.0");
    let _ = map.step(id_aligned, SLOT, 2.0, AnimationSpec::SPRING, 0.0);
    let v_after = *spring_velocity(&map.rows[&(id_aligned, SLOT)]);
    assert_eq!(v_after, v_before, "aligned retarget must preserve velocity");

    let id_opposed = wid("opposed");
    let _ = map.step(id_opposed, SLOT, 0.0, AnimationSpec::SPRING, 0.016);
    for _ in 0..3 {
        let _ = map.step(id_opposed, SLOT, 1.0, AnimationSpec::SPRING, 0.016);
    }
    assert!(
        *spring_velocity(&map.rows[&(id_opposed, SLOT)]) > 0.0,
        "precondition: moving toward 1.0"
    );
    let _ = map.step(id_opposed, SLOT, -1.0, AnimationSpec::SPRING, 0.0);
    assert_eq!(
        *spring_velocity(&map.rows[&(id_opposed, SLOT)]),
        0.0,
        "opposing retarget must zero velocity to kill reversal overshoot",
    );
}

/// The settle bound is the spring's energy `x² + v²/k` in the type's own tolerance:
/// no later swing can leave it.
///
/// - `f32` settles at 1e-4: 0.5e-4 displacement is in; speed `√170·0.9e-4` swings
///   0.9e-4, in; `1.1e-4` out; 0.8e-4 plus a 0.8e-4 swing is `0.8² + 0.8² = 1.28 >
///   1`, out.
/// - `RgbaF32` settles at 1/4096 ≈ 2.44e-4, so 2e-4 is in on a colour, out on an
///   `f32`. A colour crossing its target at 0.05/s swings `0.05/√300 ≈ 2.9e-3` and
///   stays out.
#[test]
fn settling_waits_for_the_swing_to_fall_under_the_type_tolerance() {
    let k = 170.0_f32;
    for (label, x, v, settled) in [
        ("small offset", 0.5e-4, 0.0, true),
        ("small swing", 0.0, k.sqrt() * 0.9e-4, true),
        ("large swing", 0.0, k.sqrt() * 1.1e-4, false),
        ("both near", 0.8e-4, k.sqrt() * 0.8e-4, false),
        ("f32 at a colour's tolerance", 2e-4, 0.0, false),
    ] {
        assert_eq!(spring::within_settle_eps(x, v, k), settled, "{label}");
    }
    let red = |r: f32| RgbaF32::new(r, 0.0, 0.0, 0.0);
    assert!(
        spring::within_settle_eps(red(2e-4), red(0.0), k),
        "a colour at 2e-4"
    );
    assert!(
        !spring::within_settle_eps(red(0.0), red(0.05), 300.0),
        "a bouncy colour crossing its target",
    );
}

/// A spring at `ANIM_SUBSTEP_DT` settles exactly on its target on the closed form's
/// step. Near 400 an f32 ulp is 3e-5, so feeding the rounded value back stalled
/// ~3e-4 short; the row steps its offset from the target instead.
#[test]
fn a_spring_at_the_substep_settles_on_its_target() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("substep");
    let _ = map.step(id, SLOT, 80.0, AnimationSpec::SPRING, ANIM_SUBSTEP_DT);
    let mut settled_at = None;
    for step in 0..600 {
        let r = map.step(id, SLOT, 400.0, AnimationSpec::SPRING, ANIM_SUBSTEP_DT);
        if r.settled {
            assert_eq!(r.current, 400.0);
            settled_at = Some(step);
            break;
        }
    }
    let step = closed_form_settle_step(170.0, 26.0, 320.0, 1e-4, |_| ANIM_SUBSTEP_DT);
    assert_eq!(settled_at, Some(step));
}

#[test]
fn color_spring_converges_to_target() {
    let mut map = AnimMapTyped::<RgbaF32>::default();
    let id = wid("a");
    let start = RgbaF32::srgb(0.0, 0.0, 0.0);
    let target = RgbaF32::srgb(1.0, 0.5, 0.25);
    let _ = map.step(id, SLOT, start, AnimationSpec::SPRING, 0.016);
    let mut last = start;
    let mut settled_at = None;
    for i in 0..600 {
        let r = map.step(id, SLOT, target, AnimationSpec::SPRING, 0.016);
        last = r.current;
        if r.settled {
            settled_at = Some(i);
            break;
        }
    }
    let distance = f64::from(target.sub(start).magnitude_squared()).sqrt();
    let step = closed_form_settle_step(170.0, 26.0, distance, 1.0 / 4096.0, |_| 0.016);
    assert_eq!(step, 53);
    assert_eq!(settled_at, Some(step));
    assert_eq!(last, target, "a settled spring snaps to its target");
}

#[test]
fn solid_brush_spring_matches_color_trajectory() {
    use crate::primitives::paint::brush::Brush;

    let mut color_map = AnimMapTyped::<RgbaF32>::default();
    let mut brush_map = AnimMapTyped::<Brush>::default();
    let color_id = wid("solid-color-trajectory");
    let brush_id = wid("solid-brush-trajectory");
    let start = RgbaF32::srgba(0.1, 0.2, 0.3, 0.4);
    let target = RgbaF32::srgba(0.9, 0.7, 0.5, 0.8);
    let _ = color_map.step(color_id, SLOT, start, AnimationSpec::SPRING, 0.0);
    let _ = brush_map.step(
        brush_id,
        SLOT,
        Brush::Solid(start),
        AnimationSpec::SPRING,
        0.0,
    );

    let mut settled_at = None;
    for i in 0..600 {
        let color = color_map.step(color_id, SLOT, target, AnimationSpec::SPRING, 0.016);
        let brush = brush_map.step(
            brush_id,
            SLOT,
            Brush::Solid(target),
            AnimationSpec::SPRING,
            0.016,
        );
        assert_eq!(brush.current.as_solid(), Some(color.current));
        assert_eq!(brush.settled, color.settled);
        if brush.settled {
            settled_at = Some(i);
            break;
        }
    }
    let distance = f64::from(target.sub(start).magnitude_squared()).sqrt();
    let step = closed_form_settle_step(170.0, 26.0, distance, 1.0 / 4096.0, |_| 0.016);
    assert_eq!(step, 53);
    assert_eq!(settled_at, Some(step), "both settle on the same frame");
}
