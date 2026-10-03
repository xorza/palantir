//! Damped spring transition in closed form, generic over [`Animatable`].
//! Accepted parameters converge at a bounded rate; a step costs the same
//! whatever `dt` is.

use crate::animation::animatable::Animatable;
use crate::common::time::MAX_ANIM_DT;
use std::f64::consts::PI;

pub(super) const SPRING_ERROR: &str = "spring parameters must be positive, finite, converge at 1/s or faster, and swing slower than 30 Hz";

const MIN_DECAY_RATE: f64 = 1.0;

/// The slowest panel a spring has to look right on. A spec is validated
/// when a theme is built, before any display is known, and 60 Hz is the
/// refresh rate nearly every panel runs at or above.
const MIN_DISPLAY_HZ: f64 = 60.0;

/// The fastest swing [`MIN_DISPLAY_HZ`] can show, in rad/s: its Nyquist
/// frequency, half the frame rate. A swing at or past it lands each frame
/// on an arbitrary phase, and the frames read as a slower wobble that the
/// spring does not make.
const MAX_SWING_RATE: f64 = PI * MIN_DISPLAY_HZ;

/// `(displacement, velocity)` can never again carry the value a settle
/// tolerance away from its target — the caller can snap to target and
/// clear residual motion. Single source of truth for the threshold;
/// consumed both by [`step`] and by the spring arm of the snap-if-close
/// fast path in `AnimMapTyped::tick`.
///
/// **An energy bound, not two floors.** With the target held still, the
/// spring's energy `½v² + ½k·x²` only falls — damping takes `c·v²` out of
/// it every instant and nothing puts any back — so no later `|x|` exceeds
/// `√(x² + v²/k)`, in every damping regime. That is the one quantity to
/// hold under the tolerance, measured per field by
/// [`Animatable::settle_distance_squared`]. A separate velocity floor
/// would need a ratio to position, and no fixed ratio is right: a swing
/// of amplitude `A` crosses the target at speed `A·ω`, so a floor that
/// suits a slow spring lets a bouncy one snap a visible swing, and one
/// that suits a bouncy one holds a slow one long after it stopped
/// moving. The energy bound uses the spring's own `ω² = k`.
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

/// `velocity` in the offset's unit: `v/√k`, the farthest it alone could
/// carry the value, from the energy bound on [`within_settle_eps`].
/// Scaled before anything squares it, so a stiff spring's velocity cannot
/// overflow.
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
        let discriminant = discriminant(stiffness, half_damping);
        // `ec` and `es` are `e^(-h·t)·C(t)` and `e^(-h·t)·S(t)`.
        let (ec, es) = if discriminant < 0.0 {
            let decay = (-half_damping * dt).exp();
            let psi = (-discriminant).sqrt();
            let theta = psi * dt;
            // `sin(θ)/θ` rather than a series: the library `sin` is
            // accurate to under an ulp near zero, so the quotient is
            // too, and only the removable singularity needs naming.
            let sinc = if theta == 0.0 {
                1.0
            } else {
                theta.sin() / theta
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

/// The slower of the two decay rates — how fast the spring's *last*
/// mode dies away, and so how long it takes to settle.
fn decay_rate(stiffness: f64, half_damping: f64) -> f64 {
    let discriminant = discriminant(stiffness, half_damping);
    if discriminant <= 0.0 {
        half_damping
    } else {
        stiffness / (half_damping + discriminant.sqrt())
    }
}

/// Whether a spring arrives, and whether a display can show it getting
/// there.
///
/// Its slowest mode has to decay at [`MIN_DECAY_RATE`] or faster, or the
/// motion never visibly ends. An underdamped one also has to swing slower
/// than [`MAX_SWING_RATE`]: its swing is `ψ = √(k − h²)` with `h = c/2`.
/// A critically damped or overdamped spring does not swing, so any
/// stiffness passes that half. A swing that would die within one frame
/// is refused all the same: it would read as a snap, which
/// `AnimSpec::duration(0.0, ..)` already spells.
///
/// **Not a stability bound.** The step is exact at any parameters and
/// any `dt`, so nothing here is about the arithmetic surviving. And no
/// stiffness is too stiff to settle: [`within_settle_eps`] holds the
/// energy bound under the tolerance, which falls at the decay rate
/// whatever the stiffness, so there is no velocity floor for a stiff
/// spring to sit above after its motion stopped being visible.
pub(super) fn params_are_valid(stiffness: f32, damping: f32) -> bool {
    if !(stiffness.is_finite() && stiffness > 0.0 && damping.is_finite() && damping > 0.0) {
        return false;
    }
    let (stiffness, half_damping) = (f64::from(stiffness), f64::from(damping) * 0.5);
    decay_rate(stiffness, half_damping) >= MIN_DECAY_RATE
        && -discriminant(stiffness, half_damping) < MAX_SWING_RATE * MAX_SWING_RATE
}

/// `d = h² − k`, which sorts the damping regime: below zero a spring swings
/// at `ψ = √(−d)`, at zero it is critically damped, above zero overdamped.
const fn discriminant(stiffness: f64, half_damping: f64) -> f64 {
    half_damping * half_damping - stiffness
}

/// One step of `dt` from `(offset, velocity)`, where `offset` is the
/// position measured from the target.
///
/// Stepped in offset space, where the small numbers keep their
/// precision: the caller paints `target + offset`, rounded once on the
/// way out and never fed back, so an increment under half an ulp of the
/// value still moves the offset, and the decay reaches the settle
/// tolerance however large the value is. A settled step is at rest, with
/// a zero offset and velocity.
pub(super) fn step<T: Animatable>(
    stiffness: f32,
    damping: f32,
    offset: T,
    velocity: T,
    dt: f32,
) -> SpringStep<T> {
    debug_assert!(dt.is_finite() && (0.0..=MAX_ANIM_DT).contains(&dt));
    let transition = SpringTransition::new(stiffness, damping, dt);
    // `T: Animatable` is `Clone` (not `Copy`) so heavyweights like
    // `Background` only `Copy` when their fields actually are. For the
    // common scalar/vector animations these clones compile to noops;
    // for the few wide types they're explicit by design.
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
