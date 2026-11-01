//! Vocabulary for "things that can animate." An `Animatable` type supports
//! interpolation, spring displacement arithmetic, and a squared distance in its
//! own settle tolerance; value-dependent snap-only fields normalize first.
//! Built-in impls cover `f32`, `Vec2`, arrays and `RgbaF32`; domain types opt in via
//! `#[derive(Animatable)]` (`palantir-anim-derive`, type-erased `AnimationMap`).

use crate::primitives::math::domain::EPS;
use glam::Vec2;

/// Math-only trait; storage is the type-erased `AnimationMap` keyed on `TypeId`.
///
/// The arithmetic is linear, and a spring runs it on values in motion space
/// ([`Self::to_motion`]): its offset from the target and its velocity live there, and the
/// painted value is `from_motion(target.to_motion().add(offset))`. Motion space is the value
/// itself unless a type blends in another one, as `RgbaF32` does in premultiplied Oklab.
///
/// `PartialEq` lets `tick` skip the sub + magnitude pair when the target is
/// unchanged.
///
/// `Clone`, not `Copy`: heavy types (`Chrome`, `Brush`, `Stroke`) ride the
/// recording chain once per chromed widget per frame, where auto-`Copy` was
/// measurably costly. A `Copy` supertrait would bring that back, so copies
/// spell `.clone()`; small `Copy` types still cost nothing. Sizes are pinned
/// by `hot_struct_sizes_are_pinned`.
pub trait Animatable: Clone + PartialEq + 'static {
    /// Interpolate from `a` to `b` at phase `t`, normally `0.0..=1.0`, returning `a` at 0 and
    /// `b` at 1 exactly, so a tween ends on its target. A curve may overshoot, so
    /// implementations must not clamp.
    fn lerp(a: Self, b: Self, t: f32) -> Self;
    #[must_use]
    /// Difference `self - other`.
    fn sub(self, other: Self) -> Self;
    /// Componentwise sum, the inverse of [`Self::sub`].
    #[must_use]
    fn add(self, other: Self) -> Self;
    /// Componentwise multiplication by a scalar.
    #[must_use]
    fn scale(self, k: f32) -> Self;
    /// Squared Euclidean length in the type's own unit (`self * self` for
    /// scalars, `dot(self, self)` for vectors, summed components for derives).
    fn magnitude_squared(self) -> f32;
    /// Squared length in units of this type's settle tolerance: under `1.0` a
    /// displacement is too small to see and motion may end on its target.
    ///
    /// **Unit-free by default:** an `f32` may be pixels or a 0..1 fraction, so the
    /// default divides [`Self::magnitude_squared`] by `EPS²` (`EPS = 1e-4`). A type
    /// that knows its unit says so: `RgbaF32` settles at `2^-14` in motion space, under
    /// the shortest step one 8-bit sRGB code makes there.
    ///
    /// The derive sums its fields' own distances, so pixel and color fields keep
    /// their own tolerances. A derive with one unit of its own uses
    /// `#[animate(settle_eps = ...)]`.
    #[inline]
    fn settle_distance_squared(self) -> f32 {
        self.magnitude_squared() / (EPS * EPS)
    }
    /// The additive identity: zero displacement and a fresh spring's velocity.
    fn zero() -> Self;

    /// `self` in the space motion runs in, where [`Self::sub`], [`Self::add`] and
    /// [`Self::scale`] blend as the type means to. The identity unless overridden.
    #[inline]
    #[must_use]
    fn to_motion(self) -> Self {
        self
    }

    /// The value at `motion`, back from motion space: the inverse of [`Self::to_motion`].
    #[inline]
    #[must_use]
    fn from_motion(motion: Self) -> Self {
        motion
    }

    /// Normalize fields that cannot participate in spring arithmetic.
    ///
    /// Runs when a spring takes a new target or a row becomes a spring. Between
    /// those the value is `from_motion(target.to_motion().add(offset))`, so an
    /// [`Self::add`] that keeps `self` for such a field keeps it compatible.
    /// Compound derives forward this fieldwise; implementations may install the
    /// target and clear only their matching velocity. `self` is a value and
    /// `velocity` is in motion space.
    #[inline]
    fn normalize_for_spring(&mut self, _target: &Self, _velocity: &mut Self) {}
}

// Per type, not a blanket over `Add + Sub + Mul<f32>`, which would claim every
// such type: `Animatable` is each type's decision.
/// `a + (b − a)·t`, which can miss `b` at `t = 1` by a rounding and makes a NaN of two equal
/// infinities, so both ends and an equal pair return an end.
impl Animatable for f32 {
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        if t == 0.0 {
            a
        } else if t == 1.0 || a == b {
            b
        } else {
            a + (b - a) * t
        }
    }
    #[inline]
    fn sub(self, other: Self) -> Self {
        self - other
    }
    #[inline]
    fn add(self, other: Self) -> Self {
        self + other
    }
    #[inline]
    fn scale(self, k: f32) -> Self {
        self * k
    }
    #[inline]
    fn magnitude_squared(self) -> f32 {
        self * self
    }
    #[inline]
    fn zero() -> Self {
        0.0
    }
}

impl Animatable for Vec2 {
    /// Per axis, as `f32` blends.
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self::new(f32::lerp(a.x, b.x, t), f32::lerp(a.y, b.y, t))
    }
    #[inline]
    fn sub(self, other: Self) -> Self {
        self - other
    }
    #[inline]
    fn add(self, other: Self) -> Self {
        self + other
    }
    #[inline]
    fn scale(self, k: f32) -> Self {
        self * k
    }
    #[inline]
    fn magnitude_squared(self) -> f32 {
        self.length_squared()
    }
    #[inline]
    fn zero() -> Self {
        Self::ZERO
    }
}

/// Element by element, each settling in its own unit, as a derive sums its fields.
impl<T: Animatable, const N: usize> Animatable for [T; N] {
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        let mut b = b.into_iter();
        a.map(|a| T::lerp(a, b.next().expect("arrays of one length"), t))
    }
    #[inline]
    fn sub(self, other: Self) -> Self {
        let mut other = other.into_iter();
        self.map(|x| x.sub(other.next().expect("arrays of one length")))
    }
    #[inline]
    fn add(self, other: Self) -> Self {
        let mut other = other.into_iter();
        self.map(|x| x.add(other.next().expect("arrays of one length")))
    }
    #[inline]
    fn scale(self, k: f32) -> Self {
        self.map(|x| x.scale(k))
    }
    #[inline]
    fn magnitude_squared(self) -> f32 {
        self.into_iter().map(T::magnitude_squared).sum()
    }
    #[inline]
    fn settle_distance_squared(self) -> f32 {
        self.into_iter().map(T::settle_distance_squared).sum()
    }
    #[inline]
    fn zero() -> Self {
        std::array::from_fn(|_| T::zero())
    }
    #[inline]
    fn to_motion(self) -> Self {
        self.map(T::to_motion)
    }
    #[inline]
    fn from_motion(motion: Self) -> Self {
        motion.map(T::from_motion)
    }
    #[inline]
    fn normalize_for_spring(&mut self, target: &Self, velocity: &mut Self) {
        for ((value, target), velocity) in self.iter_mut().zip(target).zip(velocity) {
            value.normalize_for_spring(target, velocity);
        }
    }
}

// `RgbaF32` blends in premultiplied Oklab (see `primitives/paint/color/mod.rs`).
//
// No `Option<T>` blanket: what an absent value blends through is each type's own answer. A
// chrome that is absent paints as `Chrome::NONE`, so `Option<Chrome>` blends through it; an
// absent text override is the ambient style, which only the theme holds. Elsewhere "absent or
// value" is a sentinel (`Stroke::NONE`) that the paint-time `is_noop` filter drops.

#[cfg(test)]
mod tests {
    use crate::primitives::math::animatable::Animatable;
    use glam::Vec2;

    /// `0.32 + (1.32 − 0.32)·1` rounds to `1.3199999`, one ulp short of `1.32`, so the end needs
    /// its own case; two equal infinities would make `∞ − ∞ = NaN`. Between the ends the blend is
    /// linear.
    #[test]
    fn scalars_end_exactly_and_keep_an_equal_pair() {
        assert_ne!(0.32_f32 + (1.32_f32 - 0.32) * 1.0, 1.32);
        assert_eq!(f32::lerp(0.32, 1.32, 1.0), 1.32);
        assert_eq!(f32::lerp(0.32, 1.32, 0.0), 0.32);
        assert_eq!(f32::lerp(f32::INFINITY, f32::INFINITY, 0.3), f32::INFINITY);
        // 2 + (10 − 2)·¼ = 4.
        assert_eq!(f32::lerp(2.0, 10.0, 0.25), 4.0);
        assert_eq!(
            Vec2::lerp(Vec2::new(0.32, 2.0), Vec2::new(1.32, 10.0), 1.0),
            Vec2::new(1.32, 10.0)
        );
        assert_eq!(
            Vec2::lerp(Vec2::new(0.0, 2.0), Vec2::new(4.0, 10.0), 0.25),
            Vec2::new(1.0, 4.0)
        );
    }

    /// An array blends, adds and scales element by element; its magnitude sums them.
    #[test]
    fn arrays_work_element_by_element() {
        let a = [Vec2::new(0.0, 4.0), Vec2::new(2.0, 2.0)];
        let b = [Vec2::new(4.0, 0.0), Vec2::new(2.0, 6.0)];
        // (0, 4) + ((4, 0) − (0, 4))·¼ = (1, 3); (2, 2) + (0, 4)·¼ = (2, 3).
        assert_eq!(
            <[Vec2; 2]>::lerp(a, b, 0.25),
            [Vec2::new(1.0, 3.0), Vec2::new(2.0, 3.0)]
        );
        assert_eq!(b.sub(a), [Vec2::new(4.0, -4.0), Vec2::new(0.0, 4.0)]);
        assert_eq!(a.add(b.sub(a)), b);
        assert_eq!(a.scale(0.5), [Vec2::new(0.0, 2.0), Vec2::new(1.0, 1.0)]);
        // 0² + 4² + 2² + 2² = 24.
        assert_eq!(a.magnitude_squared(), 24.0);
        assert_eq!(<[Vec2; 2]>::zero(), [Vec2::ZERO; 2]);
    }
}
