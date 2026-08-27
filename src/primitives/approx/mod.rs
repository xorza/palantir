//! Float comparison at UI tolerance, and the two canonicalizations that
//! tolerance splits into.
//!
//! One epsilon ([`EPS`]) answers "can the eye resolve this", and every
//! predicate here is that comparison under a name: is this zero, does this
//! paint, do these coincide, what share is this. [`FloatHash`] carries the
//! same question into a hasher, where equality-compatible and
//! visual-identity canonicalization part ways.

use glam::Vec2;
use std::hash::Hasher;

/// Float comparisons at UI tolerance.
///
/// `EPS = 1e-4` is below 8-bit color precision (1/255 ≈ 4e-3) and sub-pixel
/// position resolution at typical display scales, so differences smaller
/// than this are invisible to the user.
pub(crate) const EPS: f32 = 1.0e-4;

/// True if `c` is within `EPS` of zero.
#[inline]
pub(crate) const fn approx_zero(c: f32) -> bool {
    c.abs() <= EPS
}

/// Equality-compatible bits for public `Hash` implementations. Rust float
/// equality treats both signed zeros as equal, so they must share one hash;
/// every other value retains its exact representation.
#[inline]
pub(crate) const fn eq_bits(f: f32) -> u32 {
    if f == 0.0 { 0 } else { f.to_bits() }
}

/// Canonicalize an `f32` at visual content-cache boundaries: collapse values
/// visually indistinguishable from zero to one bit pattern and every NaN to
/// one quiet NaN. Values outside the zero tolerance retain their exact bits.
#[inline]
pub(crate) const fn canon_bits(f: f32) -> u32 {
    if f.is_nan() {
        f32::NAN.to_bits()
    } else if approx_zero(f) {
        0u32
    } else {
        f.to_bits()
    }
}

/// Feeding a value to a hasher under one of the two float tolerances this
/// crate keeps apart.
///
/// - [`hash_eq`](Self::hash_eq) is the `Hash` half of `Hash`/`PartialEq`
///   agreement: only the signed zeros are folded together, because only
///   they compare equal.
/// - [`hash_visual`](Self::hash_visual) is content-cache identity: a
///   difference the eye cannot resolve must not split a cache key, so
///   sub-`EPS` magnitudes collapse to one zero and every NaN to one
///   quiet NaN.
///
/// A trait rather than a `hash_visual_{f32,vec2,size,rect}` family: the
/// suffix was type dispatch spelled by hand, and it kept a type's second
/// hashing policy in a module the type knows nothing about — while its
/// first sat in its own `Hash` impl.
pub(crate) trait FloatHash {
    /// Feed `self` under equality-compatible canonicalization.
    fn hash_eq<H: Hasher>(&self, state: &mut H);

    /// Feed `self` under visual canonicalization.
    fn hash_visual<H: Hasher>(&self, state: &mut H);
}

impl FloatHash for f32 {
    #[inline]
    fn hash_eq<H: Hasher>(&self, state: &mut H) {
        state.write_u32(eq_bits(*self));
    }

    #[inline]
    fn hash_visual<H: Hasher>(&self, state: &mut H) {
        state.write_u32(canon_bits(*self));
    }
}

/// Both lanes in one `write_u64` rather than two `hash_eq` calls on the
/// components — one hasher round per point, on a path that runs per vertex
/// and per shape.
impl FloatHash for Vec2 {
    #[inline]
    fn hash_eq<H: Hasher>(&self, state: &mut H) {
        state.write_u64(((eq_bits(self.x) as u64) << 32) | eq_bits(self.y) as u64);
    }

    #[inline]
    fn hash_visual<H: Hasher>(&self, state: &mut H) {
        state.write_u64(((canon_bits(self.x) as u64) << 32) | canon_bits(self.y) as u64);
    }
}

/// True if `v` would produce no visible paint when used as a
/// magnitude (stroke width, alpha, etc.). Captures three cases in
/// one comparison: `v <= EPS` is true for near-zero positives,
/// exact zero, and any negative; the `is_nan` branch handles the
/// NaN case (NaN compares false against everything). Useful as the
/// shared predicate behind `Stroke::is_noop`, `Color::is_noop`,
/// and per-variant `Shape::is_noop` checks — keeps the
/// "non-paintable scalar" contract in one place.
///
/// "Does this paint anything?" is asked at two tiers, and they compose
/// rather than duplicate. `is_paint_empty` is the geometry half — does
/// this `Size` / `Rect` / `URect` / `QuadGeom` cover any pixels at all —
/// and bottoms out here. `is_noop` is the whole question, and a type
/// that carries both geometry and paint answers it by calling the first
/// and then testing its ink; `DrawQuadPayload` carries the pair and
/// reads that way.
#[inline]
pub(crate) const fn noop_f32(v: f32) -> bool {
    v.is_nan() || v <= EPS
}

/// `n / d`, or zero when `d` carries no paintable magnitude.
///
/// The one answer to "what share of `d` is `n`" for a `d` that geometry
/// can legitimately collapse — a scroll range with nothing to scroll, a
/// bar whose thumb fills its track, a line height with no font behind
/// it, a slider rail narrower than its own knob. Flooring the divisor at
/// a tolerance instead returns an enormous number for a quantity every
/// caller then reads as a fraction: a wrong answer stated confidently.
///
/// The gate is [`noop_f32`], so a *negative* `d` is degenerate too and
/// not merely a sign flip. Every `d` this divides is a distance, and one
/// that came out backwards has no share to report any more than a zero
/// one does — the negated share puts a splitter rule on the wrong side
/// of a track narrower than its own bar.
#[inline]
pub(crate) const fn ratio(n: f32, d: f32) -> f32 {
    if noop_f32(d) { 0.0 } else { n / d }
}

/// True if two 2D points are within `EPS` of each other (Euclidean
/// distance). Compares squared distance against `EPS²` to avoid a
/// `sqrt`. Use when two points should be treated as coincident
/// (degenerate stroke endpoints, zero-length segments).
#[inline]
pub(crate) const fn vec2_approx_eq(a: glam::Vec2, b: glam::Vec2) -> bool {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy <= EPS * EPS
}

#[cfg(test)]
mod tests;
