//! Damped spring transition in closed form, generic over [`Animatable`].
//! Accepted parameters converge at a bounded rate; a step costs the same
//! whatever `dt` is.

use crate::animation::animatable::Animatable;
use crate::common::time::MAX_ANIM_DT;
use std::f64::consts::PI;

pub(super) const SPRING_ERROR: &str = "spring parameters must be positive, finite, converge at 1/s or faster, and swing slower than 30 Hz";

const MIN_DECAY_RATE: f64 = 1.0;

/// The slowest panel a spring has to look right on: specs are validated before
/// any display is known, and nearly every panel runs at 60 Hz or above.
const MIN_DISPLAY_HZ: f64 = 60.0;

/// The fastest swing [`MIN_DISPLAY_HZ`] can show, in rad/s (its Nyquist
/// frequency). Past it each frame lands on an arbitrary phase and reads as a
/// slower wobble the spring does not make.
const MAX_SWING_RATE: f64 = PI * MIN_DISPLAY_HZ;

/// `(displacement, velocity)` can never again carry the value a settle
/// tolerance away from its target, so the caller can snap and clear residual
/// motion. The one threshold, used by [`step`] and the spring arm of
/// `AnimMapTyped::tick`'s snap-if-close path.
///
/// An energy bound, not two floors. With the target held still the energy
/// `½v² + ½k·x²` only falls, so no later `|x|` exceeds `√(x² + v²/k)` in any
/// damping regime; that is the quantity held under the tolerance, measured by
/// [`Animatable::settle_distance_squared`]. No fixed velocity-to-position
/// floor suits both slow and bouncy springs, since a swing of amplitude `A`
/// crosses the target at `A·ω`; the bound uses the spring's own `ω² = k`.
#[inline]
pub(super) fn within_settle_eps<T: Animatable>(
    displacement: T,
    velocity: T,
    stiffness: f32,
) -> bool {
    displacement.settle_distance_squared()
        + velocity_reach(velocity, stiffness).settle_distance_squared()
        < 1.0
}

/// `velocity` in the offset's unit: `v/√k`, the farthest it alone could carry
/// the value (see [`within_settle_eps`]). Scaled before squaring so a stiff
/// spring cannot overflow.
#[inline]
pub(super) fn velocity_reach<T: Animatable>(velocity: T, stiffness: f32) -> T {
    velocity.scale(stiffness.sqrt().recip())
}

#[derive(Debug)]
pub(super) struct SpringStep<T: Animatable> {
    pub(super) offset: T,
    pub(super) velocity: T,
    pub(super) settled: bool,
}

/// How one step of `dt` maps `(displacement, velocity)` onto the next pair:
/// the exact solution of `x'' + damping·x' + stiffness·x = 0` for a target
/// held still across the step.
///
/// A transition, not an integrator. Substepped Euler is approximate, depends
/// on how frames were partitioned, and needs a stability-bound substep the
/// spring model does not ask for. The exact solution makes one call as exact
/// as a thousand, for two `exp` and a `sin` or `expm1`.
///
/// With `h = damping/2` and `d = h² - stiffness`:
///
/// ```text
/// x(t) = e^(-h·t)·[ x₀·C(t) + (v₀ + h·x₀)·S(t) ]
/// v(t) = e^(-h·t)·[ v₀·C(t) − (stiffness·x₀ + h·v₀)·S(t) ]
/// ```
///
/// where `C` and `S` are the even and odd entire functions
/// `C = Σ (d·t²)ⁿ/(2n)!` and `S = t·Σ (d·t²)ⁿ/(2n+1)!`, with `C' = d·S` and
/// `S' = C`. Underdamped, critical and overdamped are the `d < 0`, `d = 0`
/// and `d > 0` points of one pair (`C` is `cos`, `1`, `cosh`; `S` is
/// `sin(ψt)/ψ`, `t`, `sinh(ψt)/ψ`). Splitting them, as Ryan Juckett's
/// derivation does, needs an epsilon around `d = 0`; here critical damping is
/// the `ψ = 0` point of a branch accurate through it.
///
/// The branches exist for arithmetic: both carry `e^(-h·t)` inside, since
/// `cosh(ψt)` alone overflows `f64` past `ψt ≈ 710` (reachable for a stiff
/// overdamped spring, `ψ` just under `h`). The exponential branch spells `S`
/// through `expm1`, exact where `1 − e^(-2ψt)` would cancel.
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
        let discriminant = discriminant(stiffness, half_damping);
        // `ec` and `es` are `e^(-h·t)·C(t)` and `e^(-h·t)·S(t)`.
        let (ec, es) = if discriminant < 0.0 {
            let decay = (-half_damping * dt).exp();
            let psi = (-discriminant).sqrt();
            let theta = psi * dt;
            // `sin(θ)/θ`, not a series: `sin` is accurate near zero, so only
            // the removable singularity needs naming.
            let sinc = if theta == 0.0 {
                1.0
            } else {
                theta.sin() / theta
            };
            (decay * theta.cos(), decay * dt * sinc)
        } else {
            let psi = discriminant.sqrt();
            let theta = psi * dt;
            // The slow root's decay `e^(-(h-ψ)t)` scales the product; `ψ < h`
            // for positive stiffness, so it never exceeds 1.
            let slow = ((psi - half_damping) * dt).exp();
            let shrink = (-2.0 * theta).exp_m1();
            let sinhc = if theta == 0.0 {
                1.0
            } else {
                -shrink / (2.0 * theta)
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

/// The slower of the two decay rates: how fast the last mode dies, and so how
/// long the spring takes to settle.
fn decay_rate(stiffness: f64, half_damping: f64) -> f64 {
    let discriminant = discriminant(stiffness, half_damping);
    if discriminant <= 0.0 {
        half_damping
    } else {
        stiffness / (half_damping + discriminant.sqrt())
    }
}

/// Whether a spring arrives and a display can show it getting there.
///
/// Its slowest mode must decay at [`MIN_DECAY_RATE`] or faster, or the motion
/// never visibly ends. An underdamped one must also swing slower than
/// [`MAX_SWING_RATE`], where the swing is `ψ = √(k − h²)` with `h = c/2`; a
/// critically damped or overdamped spring does not swing.
///
/// Not a stability bound: the step is exact at any parameters and `dt`, and no
/// stiffness is too stiff to settle, since [`within_settle_eps`]'s energy
/// bound falls at the decay rate.
pub(super) fn params_are_valid(stiffness: f32, damping: f32) -> bool {
    if !(stiffness.is_finite() && stiffness > 0.0 && damping.is_finite() && damping > 0.0) {
        return false;
    }
    let (stiffness, half_damping) = (f64::from(stiffness), f64::from(damping) * 0.5);
    decay_rate(stiffness, half_damping) >= MIN_DECAY_RATE
        && -discriminant(stiffness, half_damping) < MAX_SWING_RATE * MAX_SWING_RATE
}

/// `d = h² − k`, which sorts the regime: below zero a spring swings at
/// `ψ = √(−d)`, at zero it is critically damped, above zero overdamped.
const fn discriminant(stiffness: f64, half_damping: f64) -> f64 {
    half_damping * half_damping - stiffness
}

/// One step of `dt` from `(offset, velocity)`, where `offset` is measured from
/// the target. Stepped in offset space, where small numbers keep their
/// precision: the caller paints `target + offset`, rounded once and never fed
/// back, so the decay reaches the settle tolerance however large the value is.
/// A settled step is at rest, with zero offset and velocity.
pub(super) fn step<T: Animatable>(
    stiffness: f32,
    damping: f32,
    offset: T,
    velocity: T,
    dt: f32,
) -> SpringStep<T> {
    debug_assert!(dt.is_finite() && (0.0..=MAX_ANIM_DT).contains(&dt));
    let transition = SpringTransition::new(stiffness, damping, dt);
    // `T: Animatable` is `Clone`, not `Copy`, so heavyweights like `Background`
    // are only `Copy` when their fields are; scalar clones compile to noops.
    let moved = offset
        .clone()
        .scale(transition.pos_from_pos)
        .add(velocity.clone().scale(transition.pos_from_vel));
    let velocity = velocity
        .scale(transition.vel_from_vel)
        .add(offset.scale(transition.vel_from_pos));
    if within_settle_eps(moved.clone(), velocity.clone(), stiffness) {
        return SpringStep {
            offset: T::zero(),
            velocity: T::zero(),
            settled: true,
        };
    }
    SpringStep {
        offset: moved,
        velocity,
        settled: false,
    }
}
