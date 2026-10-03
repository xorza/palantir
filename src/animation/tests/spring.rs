//! The spring transition: its agreement with the closed-form solution,
//! its independence from how frames partition an interval, and how a row
//! carrying one settles.

use crate::animation::anim_spec::AnimMotion;
use crate::animation::easing::Easing;
use crate::animation::tests::support::{
    SLOT, closed_form_settle_step, duration_motion, spring_velocity, wid,
};
use crate::animation::*;
use crate::common::time::ANIM_SUBSTEP_DT;
use crate::primitives::math::domain::internals::assert_close;
use crate::primitives::paint::color::RgbaF32;

/// Accepted springs stay finite and settle on their target under a mixed
/// frame sequence, on the step the closed form names. Step 0 is the
/// change's frame, which steps nothing, so step `n` spends `dts[n % 4]`.
#[test]
fn validated_springs_remain_finite_and_settle() {
    // (label, spec, settle step)
    let cases = [
        ("minimum-decay", AnimSpec::spring(1.0, 2.0), 488),
        ("default", AnimSpec::SPRING, 39),
        (
            "stiff, critically damped",
            AnimSpec::spring(1_000_000.0, 2_000.0),
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

/// The three damping regimes against the solution of
/// `x'' + c·x' + k·x = 0` computed independently, plus the stiff
/// overdamped corner where a textbook `e^(-h·t)·cosh(ψ·t)` overflows
/// `f64` long before the product it belongs to leaves `[0, 1]`.
///
/// Released from rest at `x₀ = 1`, so `x(t) = e^(-h·t)(C + h·S)` and
/// `v(t) = -k·e^(-h·t)·S`, with `h = c/2` and `(C, S)` the pair named on
/// `SpringTransition`. Values below come from evaluating that by hand
/// per regime: `cos`/`sin` for `k=5, c=2` (roots `-1 ± 2i`), the `ψ = 0`
/// limit for `k=100, c=20` (`e^-1·(1 + 10t)`), and `(4/3)e^(-t) −
/// (1/3)e^(-4t)` for `k=4, c=5` (roots `-1` and `-4`).
#[test]
fn closed_form_matches_the_analytic_solution_in_every_damping_regime() {
    // (label, stiffness, damping, expected position, expected velocity).
    let cases = [
        ("underdamped", 5.0, 2.0, 0.976_682_66, -0.449_408_62),
        ("critically damped", 100.0, 20.0, 0.735_758_9, -3.678_794_4),
        ("overdamped", 4.0, 5.0, 0.983_009_9, -0.312_689_84),
        // ψ·dt ≈ 995, where `cosh` alone is 1e432 — `f64` holds it,
        // but only just, and one step stiffer it would not.
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

/// The property the closed form buys: for a target held still, the
/// travel over an interval does not depend on how the frames cut it up.
/// A substepped Euler integrator fails this by construction — it
/// re-partitions at every call — and the gap it leaves is far wider than
/// the f32 rounding this tolerates.
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
    // `(170, 26)` is barely underdamped (ψ = 1), so 300 px becomes
    // `300·e^(-1.3)·(cos 0.1 + 13·sin 0.1)` ≈ 187 px after 0.1 s: still
    // in flight, and four orders of magnitude above the agreement
    // asserted below.
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

/// Critical damping is the `ψ = 0` point of the exponential branch, not
/// a third case guarded by a tolerance, so the transition is continuous
/// across `damping = 2√stiffness` — where a split-by-epsilon
/// implementation has a seam whose width is the epsilon.
#[test]
fn the_critically_damped_boundary_has_no_seam() {
    let stiffness = 100.0_f32;
    let at = |damping: f32| spring::step(stiffness, damping, 1.0_f32, 0.0, 0.1).offset;
    let critical = at(20.0);
    for offset in [1e-3, 1e-4, 1e-5] {
        let under = at(20.0 - offset);
        let over = at(20.0 + offset);
        // Near critical, x(0.1) moves about 0.0061 per unit of damping:
        // `∂x/∂c = −t²·h·e^(−ht)/2 + e^(−ht)(t²/2 + h·t³/6)·h`, h = 10,
        // t = 0.1. The gap shrinks with the offset, down to the ulp of
        // 0.74; a seam would hold its width.
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
    let custom = AnimSpec::spring(100.0, 15.0);
    let _ = default_map.step(id, SLOT, 0.0, AnimSpec::SPRING, 0.016);
    let _ = custom_map.step(id, SLOT, 0.0, custom, 0.016);
    // The change's frame starts from rest and moves neither.
    let _ = default_map.step(id, SLOT, 1.0, AnimSpec::SPRING, 0.016);
    let _ = custom_map.step(id, SLOT, 1.0, custom, 0.016);
    let default = default_map
        .step(id, SLOT, 1.0, AnimSpec::SPRING, 0.016)
        .current;
    let custom = custom_map.step(id, SLOT, 1.0, custom, 0.016).current;
    assert_ne!(default, custom);
}

/// Worst-case wall-clock `dt` — `MAX_ANIM_DT`, after a stalled frame or
/// a tab-switch redraw gap — must not throw the value past its
/// endpoints. A single Euler step at `dt = 0.1` with the default spring
/// `(170, 26)` used to land `current` far past the target, negative for
/// the showcase animation widths, which trips the `Sizing::fixed`
/// invariant. Pin: stepping a 400→80 spring with `dt = 0.1` keeps
/// `current` within `[80, 400]`.
#[test]
fn spring_step_at_max_dt_stays_bounded() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let _ = map.step(id, SLOT, 400.0, AnimSpec::SPRING, 0.016);
    // From rest, the change's frame steps nothing; the next one spends
    // the whole clamp.
    let r = map.step(id, SLOT, 80.0, AnimSpec::SPRING, 0.1);
    assert_eq!(r.current, 400.0);
    let r = map.step(id, SLOT, 80.0, AnimSpec::SPRING, 0.1);
    // Closed form, released from rest 320 px out: `(170, 26)` has h = 13
    // and ω = 1, so `80 + 320·e^(-1.3)(cos 0.1 + 13 sin 0.1)` ≈ 279.96,
    // inside [80, 400].
    let t = f64::from(0.1f32);
    let expected = 80.0 + 320.0 * (-13.0 * t).exp() * (t.cos() + 13.0 * t.sin());
    assert_close(r.current, expected, 1e-4, "a few f32 ulps at 280");
}

/// A frame may run `build` twice on input frames (pass A
/// records, drains input, pass B re-records with the post-action
/// state). Both passes call `Ui::animate`, which dispatches to
/// `tick`. The multi-pass guard keys on `render_frame_id` so two ticks
/// sharing one — i.e. one wall-clock frame — only advance the
/// integrator once. Retargets in pass B must still take effect (the
/// next frame should ease toward the new target from pass A's
/// advanced position), but the second tick must not add another
/// `dt` of motion.
#[test]
fn second_tick_in_same_frame_does_not_double_advance() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let frame = 42;

    // Seed: row settled at 0.0. Different frame so we don't trip the
    // guard during setup.
    let _ = map.tick(id, SLOT, 0.0, AnimSpec::FAST, 0.016, frame - 2);

    // A retarget from rest steps nothing in either pass: pass A and pass
    // B agree on the start.
    let from_rest_a = map.tick(id, SLOT, 1.0, AnimSpec::FAST, 0.016, frame - 1);
    let from_rest_b = map.tick(id, SLOT, 1.0, AnimSpec::FAST, 0.016, frame - 1);
    assert_eq!((from_rest_a.current, from_rest_b.current), (0.0, 0.0));

    // Pass A: in flight now, advance one step.
    let pass_a = map.tick(id, SLOT, 1.0, AnimSpec::FAST, 0.016, frame);
    let eased = Easing::OutCubic.apply(0.016 / 0.12);
    assert_eq!(pass_a.current, eased, "FAST: 16 ms of 120 ms, out-cubic");
    let pass_a_current = pass_a.current;
    let pass_a_elapsed = duration_motion(&map.rows[&(id, SLOT)]).elapsed;

    // Pass B: same render_frame_id, same target. Must NOT advance further;
    // current and elapsed must match pass A exactly.
    let pass_b = map.tick(id, SLOT, 1.0, AnimSpec::FAST, 0.016, frame);
    assert_eq!(
        pass_b.current, pass_a_current,
        "pass B with same render_frame_id must not advance current",
    );
    assert_eq!(
        duration_motion(&map.rows[&(id, SLOT)]).elapsed,
        pass_a_elapsed,
        "pass B with same render_frame_id must not advance elapsed",
    );

    // Pass B with a *different* target (post-action retarget): the
    // segment resets so the next frame eases toward the new target,
    // but the current value is held at pass A's advanced position.
    let pass_b_retarget = map.tick(id, SLOT, 5.0, AnimSpec::FAST, 0.016, frame);
    assert_eq!(
        pass_b_retarget.current, pass_a_current,
        "retargeting in pass B updates segment but doesn't re-step",
    );
    assert_eq!(map.rows[&(id, SLOT)].target, 5.0);
    assert_eq!(
        *duration_motion(&map.rows[&(id, SLOT)]).segment_start,
        pass_a_current
    );

    // Next frame: integrator advances from the retargeted segment.
    let next = map.tick(id, SLOT, 5.0, AnimSpec::FAST, 0.016, frame + 1);
    assert_eq!(
        next.current,
        pass_a_current + (5.0 - pass_a_current) * eased,
        "next frame eases 16 ms toward 5.0 from pass A's current",
    );
}

/// Spring retarget into the path of motion keeps velocity (the
/// "fling-through" continuation); retarget *against* the velocity
/// zeroes it so the new segment can't swing far past the target.
/// Without the projection, a fast click-then-reverse can drag the
/// value well below zero / above any plausible bound; the
/// `Sizing::fixed` invariant in the showcase relied on this.
#[test]
fn spring_retarget_zeroes_opposing_velocity_only() {
    let mut map = AnimMapTyped::<f32>::default();

    // Aligned: moving toward 1.0, retarget further along the same
    // direction (2.0). Velocity should survive — that's the fling.
    let id_aligned = wid("aligned");
    let _ = map.step(id_aligned, SLOT, 0.0, AnimSpec::SPRING, 0.016);
    for _ in 0..3 {
        let _ = map.step(id_aligned, SLOT, 1.0, AnimSpec::SPRING, 0.016);
    }
    let v_before = *spring_velocity(&map.rows[&(id_aligned, SLOT)]);
    assert!(v_before > 0.0, "precondition: moving toward 1.0");
    let _ = map.step(id_aligned, SLOT, 2.0, AnimSpec::SPRING, 0.0);
    let v_after = *spring_velocity(&map.rows[&(id_aligned, SLOT)]);
    assert_eq!(v_after, v_before, "aligned retarget must preserve velocity");

    // Opposed: moving toward 1.0, retarget backward to -1.0. Velocity
    // points away from the new target — zero it.
    let id_opposed = wid("opposed");
    let _ = map.step(id_opposed, SLOT, 0.0, AnimSpec::SPRING, 0.016);
    for _ in 0..3 {
        let _ = map.step(id_opposed, SLOT, 1.0, AnimSpec::SPRING, 0.016);
    }
    assert!(
        *spring_velocity(&map.rows[&(id_opposed, SLOT)]) > 0.0,
        "precondition: moving toward 1.0"
    );
    let _ = map.step(id_opposed, SLOT, -1.0, AnimSpec::SPRING, 0.0);
    assert_eq!(
        *spring_velocity(&map.rows[&(id_opposed, SLOT)]),
        0.0,
        "opposing retarget must zero velocity to kill reversal overshoot",
    );
}

/// The settle bound is the spring's energy, `x² + v²/k`, measured in the
/// type's own tolerance: no later swing can leave it.
///
/// - `f32` settles at 1e-4. A displacement of 0.5e-4 is in; at the target
///   with speed `√170 · 0.9e-4` the swing still to come is 0.9e-4, in; at
///   `√170 · 1.1e-4` it is 1.1e-4, out. 0.8e-4 out with a 0.8e-4 swing
///   is in on each axis but `0.8² + 0.8² = 1.28 > 1` together, out.
/// - `RgbaF32` settles at 1/4096 ≈ 2.44e-4, so 2e-4 on one channel is in
///   where the same number on an `f32` is out — the tolerance is the
///   type's. A bouncy colour spring crossing its target at 0.05/s, which
///   an absolute 0.1 velocity floor let snap, still swings
///   `0.05 / √300 ≈ 2.9e-3` — twelve tolerances — and stays out.
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

/// A spring stepped at the smallest delta the frame runtime spends,
/// `ANIM_SUBSTEP_DT`, settles exactly on its target, on the step the
/// closed form names. At pixel-scale values the last stretch moves less
/// than f32 can show next to the value: near 400 a step of f32 is 3e-5,
/// and one substep decays the remaining travel by 5 %, so a spring that
/// fed the rounded value back stalled about 3e-4 short of the target. The
/// row steps its offset from the target instead, where the small numbers
/// keep their precision, so the decay runs on to the tolerance. Loop step
/// 0 is the change's frame and steps nothing, so loop step `n` is step
/// `n`.
#[test]
fn a_spring_at_the_substep_settles_on_its_target() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("substep");
    let _ = map.step(id, SLOT, 80.0, AnimSpec::SPRING, ANIM_SUBSTEP_DT);
    let mut settled_at = None;
    for step in 0..600 {
        let r = map.step(id, SLOT, 400.0, AnimSpec::SPRING, ANIM_SUBSTEP_DT);
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
    let _ = map.step(id, SLOT, start, AnimSpec::SPRING, 0.016);
    let mut last = start;
    let mut settled_at = None;
    for i in 0..600 {
        let r = map.step(id, SLOT, target, AnimSpec::SPRING, 0.016);
        last = r.current;
        if r.settled {
            settled_at = Some(i);
            break;
        }
    }
    // Loop frame 0 is the change's frame and steps nothing, so frame `i`
    // is step `i`.
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
    let _ = color_map.step(color_id, SLOT, start, AnimSpec::SPRING, 0.0);
    let _ = brush_map.step(brush_id, SLOT, Brush::Solid(start), AnimSpec::SPRING, 0.0);

    let mut settled_at = None;
    for i in 0..600 {
        let color = color_map.step(color_id, SLOT, target, AnimSpec::SPRING, 0.016);
        let brush = brush_map.step(
            brush_id,
            SLOT,
            Brush::Solid(target),
            AnimSpec::SPRING,
            0.016,
        );
        assert_eq!(brush.current.as_solid(), Some(color.current));
        assert_eq!(brush.settled, color.settled);
        if brush.settled {
            settled_at = Some(i);
            break;
        }
    }
    // Loop frame 0 is the change's frame and steps nothing, so frame `i`
    // is step `i`.
    let distance = f64::from(target.sub(start).magnitude_squared()).sqrt();
    let step = closed_form_settle_step(170.0, 26.0, distance, 1.0 / 4096.0, |_| 0.016);
    assert_eq!(step, 53);
    assert_eq!(settled_at, Some(step), "both settle on the same frame");
}
