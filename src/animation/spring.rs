//! Damped spring transition in closed form, generic over [`Animatable`].
//! Accepted parameters converge at a bounded rate; a step costs the same
//! whatever `dt` is.

use crate::animation::animatable::Animatable;
use crate::common::time::MAX_ANIM_DT;

pub(super) const SPRING_ERROR: &str = "spring parameters must be positive, finite, convergent, and settle without a long velocity tail";

const MIN_DECAY_RATE: f64 = 1.0;

/// How long [`VEL_EPS`] may keep a spring unsettled after [`POS_EPS`]
/// is met — the seconds of repaint bought by a stiffness whose motion
/// has already become invisible.
const MAX_VELOCITY_TAIL_SECS: f64 = 4.0;

// Spring settle tolerances, deliberately *loose*: a spring's job is to
// converge, and the eye cannot see the last 0.01 px of travel. A
// tighter floor buys nothing at pixel scale — the f32 ULP near
// `cur ≈ 400` is already ~2.4e-5 — and costs the integrator a frame or
// two of settle. The duration path's own far tighter floor lives with
// it, in `animation::duration`.
const POS_EPS: f32 = 0.01;
const VEL_EPS: f32 = 0.1;
const POS_EPS_SQ: f32 = POS_EPS * POS_EPS;
const VEL_EPS_SQ: f32 = VEL_EPS * VEL_EPS;

/// `(displacement, velocity)` is at the spring's settle floor — the
/// caller can snap to target and clear residual motion. Single source
/// of truth for the threshold; consumed both by [`step`] and by the
/// spring arm of the snap-if-close fast path in `AnimMapTyped::tick`.
#[inline]
pub(super) fn within_settle_eps<T: Animatable>(displacement: T, velocity: T) -> bool {
    displacement.magnitude_squared() < POS_EPS_SQ && velocity.magnitude_squared() < VEL_EPS_SQ
}

#[derive(Debug)]
pub(super) struct SpringStep<T: Animatable> {
    pub(super) current: T,
    pub(super) velocity: T,
    pub(super) settled: bool,
}

/// How one step of `dt` maps `(displacement, velocity)` onto the next
/// pair — the exact solution of `x'' + damping·x' + stiffness·x = 0`
/// for a target held still across the step.
///
/// **A transition, not an integrator.** Substepped Euler answers this
/// same question approximately, and pays twice for it: the answer
/// depends on how the frames were partitioned, and the substep has to
/// stay under an explicit-integration stability bound that no part of
/// the spring model asks for. Solving the equation instead makes one
/// call as exact as a thousand, at the cost of two `exp` and a `sin` or
/// an `expm1`, and the travel over a second no longer depends on
/// whether it arrived as six frames or six hundred.
///
/// **One family, not three cases.** With `h = damping/2` and
/// `d = h² - stiffness`, the solution is
///
/// ```text
/// x(t) = e^(-h·t)·[ x₀·C(t) + (v₀ + h·x₀)·S(t) ]
/// v(t) = e^(-h·t)·[ v₀·C(t) − (stiffness·x₀ + h·v₀)·S(t) ]
/// ```
///
/// where `C` and `S` are the even and odd entire functions
/// `C = Σ (d·t²)ⁿ/(2n)!` and `S = t·Σ (d·t²)ⁿ/(2n+1)!`, satisfying
/// `C' = d·S` and `S' = C`. Underdamped, critically damped and
/// overdamped are the `d < 0`, `d = 0` and `d > 0` points of that one
/// pair, not three formulas: `C` is `cos`, `1`, `cosh` and `S` is
/// `sin(ψt)/ψ`, `t`, `sinh(ψt)/ψ`. Ryan Juckett's derivation splits
/// them and needs an epsilon around `d = 0` to keep the split from
/// dividing by a vanishing `ψ`. Here critical damping is simply the
/// `ψ = 0` point of a branch that stays accurate through it, so there
/// is no tolerance to tune and no window where the two sides disagree.
///
/// **What the branches are for is arithmetic, not physics.** Both
/// carry `e^(-h·t)` inside, because `cosh(ψt)` alone overflows `f64`
/// past `ψt ≈ 710` while the product it belongs to stays near 1 —
/// reachable, since a stiff overdamped spring puts `ψ` just under `h`.
/// The exponential branch spells `S` through `expm1`, which is exact
/// where `1 − e^(-2ψt)` would cancel away its own significand.
#[derive(Debug)]
struct SpringTransition {
    pos_from_pos: f32,
    pos_from_vel: f32,
    vel_from_pos: f32,
    vel_from_vel: f32,
}

impl SpringTransition {
    fn new(stiffness: f32, damping: f32, dt: f32) -> Self {
        let stiffness = f64::from(stiffness);
        let half_damping = f64::from(damping) * 0.5;
        let dt = f64::from(dt);
        let discriminant = half_damping * half_damping - stiffness;
        // `ec` and `es` are `e^(-h·t)·C(t)` and `e^(-h·t)·S(t)`.
        let (ec, es) = if discriminant < 0.0 {
            let decay = (-half_damping * dt).exp();
            let psi = (-discriminant).sqrt();
            let theta = psi * dt;
            // `sin(θ)/θ` rather than a series: the library `sin` is
            // accurate to under an ulp near zero, so the quotient is
            // too, and only the removable singularity needs naming.
            let sinc = match theta == 0.0 {
                true => 1.0,
                false => theta.sin() / theta,
            };
            (decay * theta.cos(), decay * dt * sinc)
        } else {
            let psi = discriminant.sqrt();
            let theta = psi * dt;
            // The slow root's decay, `e^(-(h-ψ)t)`, which is the whole
            // product's scale: `ψ < h` whenever the stiffness is
            // positive, so this never exceeds 1 however stiff the
            // spring is.
            let slow = ((psi - half_damping) * dt).exp();
            let shrink = (-2.0 * theta).exp_m1();
            let sinhc = match theta == 0.0 {
                true => 1.0,
                false => -shrink / (2.0 * theta),
            };
            (slow * (2.0 + shrink) * 0.5, slow * dt * sinhc)
        };
        Self {
            pos_from_pos: (ec + half_damping * es) as f32,
            pos_from_vel: es as f32,
            vel_from_pos: (-stiffness * es) as f32,
            vel_from_vel: (ec - half_damping * es) as f32,
        }
    }
}

/// The slower of the two decay rates — how fast the spring's *last*
/// mode dies away, and so how long it takes to settle.
fn decay_rate(stiffness: f32, damping: f32) -> f64 {
    let stiffness = f64::from(stiffness);
    let half_damping = f64::from(damping) * 0.5;
    let discriminant = half_damping * half_damping - stiffness;
    if discriminant <= 0.0 {
        half_damping
    } else {
        stiffness / (half_damping + discriminant.sqrt())
    }
}

/// Whether a spring both arrives and stops asking for frames.
///
/// Two policies, one decay rate. The rate itself has to clear
/// [`MIN_DECAY_RATE`], or the motion never visibly ends. It also has to
/// pay for the stiffness: [`within_settle_eps`] wants position *and*
/// velocity under their floors, and a spring's velocity amplitude is
/// `√stiffness` times its position amplitude, so a stiff spring reaches
/// [`POS_EPS`] a long time before [`VEL_EPS`]. That gap is
/// `ln(√stiffness · POS_EPS/VEL_EPS) / decay` seconds of repainting a
/// motion nobody can see, and [`MAX_VELOCITY_TAIL_SECS`] is what it may
/// come to.
///
/// **Not a stability bound.** The step is exact at any parameters and
/// any `dt`, so nothing here is about the arithmetic surviving. This is
/// the same question [`MIN_DECAY_RATE`] asks — does the animation end —
/// on the axis a decay rate alone cannot see.
pub(super) fn params_are_valid(stiffness: f32, damping: f32) -> bool {
    if !(stiffness.is_finite() && stiffness > 0.0 && damping.is_finite() && damping > 0.0) {
        return false;
    }
    let decay = decay_rate(stiffness, damping);
    if decay < MIN_DECAY_RATE {
        return false;
    }
    // How far above `VEL_EPS` the spring still sits when it first
    // reaches `POS_EPS`, and how long its own decay takes to close that.
    let velocity_overshoot = f64::from(stiffness).sqrt() * f64::from(POS_EPS / VEL_EPS);
    velocity_overshoot.ln().max(0.0) / decay <= MAX_VELOCITY_TAIL_SECS
}

pub(super) fn step<T: Animatable>(
    stiffness: f32,
    damping: f32,
    current: T,
    velocity: T,
    target: T,
    dt: f32,
) -> SpringStep<T> {
    debug_assert!(dt.is_finite() && (0.0..=MAX_ANIM_DT).contains(&dt));
    let transition = SpringTransition::new(stiffness, damping, dt);
    // `T: Animatable` is `Clone` (not `Copy`) so heavyweights like
    // `Background` only `Copy` when their fields actually are. For the
    // common scalar/vector animations these clones compile to noops;
    // for the few wide types they're explicit by design.
    let displacement = current.sub(target.clone());
    let moved = displacement
        .clone()
        .scale(transition.pos_from_pos)
        .add(velocity.clone().scale(transition.pos_from_vel));
    // Spring arithmetic carries an `#[animate(snap)]` field through
    // `self`, so the left operand decides where one comes from: the old
    // velocity here, and `target` below. A chain rooted at `current`
    // would ride the first-touch value frame after frame and only catch
    // up on settle.
    let velocity = velocity
        .scale(transition.vel_from_vel)
        .add(displacement.scale(transition.vel_from_pos));
    if within_settle_eps(moved.clone(), velocity.clone()) {
        SpringStep {
            current: target,
            velocity: T::zero(),
            settled: true,
        }
    } else {
        // `moved` *is* the new displacement, so adding it to `target`
        // also spares the round trip back through a subtraction.
        SpringStep {
            current: target.add(moved),
            velocity,
            settled: false,
        }
    }
}
