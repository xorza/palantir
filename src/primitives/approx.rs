use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
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

#[inline]
pub(crate) fn hash_f32<H: Hasher>(value: f32, state: &mut H) {
    state.write_u32(eq_bits(value));
}

#[inline]
pub(crate) fn hash_vec2<H: Hasher>(value: Vec2, state: &mut H) {
    state.write_u64(((eq_bits(value.x) as u64) << 32) | eq_bits(value.y) as u64);
}

#[inline]
pub(crate) fn hash_size<H: Hasher>(value: Size, state: &mut H) {
    state.write_u64(((eq_bits(value.w) as u64) << 32) | eq_bits(value.h) as u64);
}

#[inline]
pub(crate) fn hash_rect<H: Hasher>(value: Rect, state: &mut H) {
    hash_vec2(value.min, state);
    hash_size(value.size, state);
}

#[inline]
pub(crate) fn hash_visual_f32<H: Hasher>(value: f32, state: &mut H) {
    state.write_u32(canon_bits(value));
}

#[inline]
pub(crate) fn hash_visual_vec2<H: Hasher>(value: Vec2, state: &mut H) {
    state.write_u64(((canon_bits(value.x) as u64) << 32) | canon_bits(value.y) as u64);
}

#[inline]
pub(crate) fn hash_visual_size<H: Hasher>(value: Size, state: &mut H) {
    state.write_u64(((canon_bits(value.w) as u64) << 32) | canon_bits(value.h) as u64);
}

#[inline]
pub(crate) fn hash_visual_rect<H: Hasher>(value: Rect, state: &mut H) {
    hash_visual_vec2(value.min, state);
    hash_visual_size(value.size, state);
}

/// True if `v` would produce no visible paint when used as a
/// magnitude (stroke width, alpha, etc.). Captures three cases in
/// one comparison: `v <= EPS` is true for near-zero positives,
/// exact zero, and any negative; the `is_nan` branch handles the
/// NaN case (NaN compares false against everything). Useful as the
/// shared predicate behind `Stroke::is_noop`, `Color::is_noop`,
/// and per-variant `Shape::is_noop` checks — keeps the
/// "non-paintable scalar" contract in one place.
#[inline]
pub(crate) const fn noop_f32(v: f32) -> bool {
    v.is_nan() || v <= EPS
}

/// True if an f16 stored as `u16` bits is `≤ EPS` in absolute value.
/// Branch-free bit-pattern check — masks the sign bit and compares
/// directly against `EPS` as f16 bits, with no f16→f32 conversion.
/// Works because positive f16 values are monotonic in their bit
/// representation (IEEE 754 design). NaN's exponent bits land at
/// `0x7C00`+, well above the threshold, so NaN classifies as
/// non-zero — matches `Corners::approx_zero` semantics and treats
/// NaN as a loud programming bug rather than a silent skip.
#[inline]
pub(crate) const fn noop_f16_bits(bits: u16) -> bool {
    const EPS_BITS: u16 = half::f16::from_f32_const(EPS).to_bits();
    const ABS_MASK: u16 = 0x7FFF;
    (bits & ABS_MASK) <= EPS_BITS
}

/// True if an f16 stored as `u16` bits is within `EPS` below 1.0 (or
/// above). Mirror of `noop_f16_bits` for the opacity end of the
/// scale: positive f16 values are monotonic in their bit
/// representation, so `>= f16(1.0 - EPS).to_bits()` catches every
/// value visually indistinguishable from fully opaque. The upper
/// bound `< 0x7C00` rejects NaN (NaN exponent starts at `0x7C01`+)
/// — a NaN alpha is a loud bug, not a silent opaque pass. Negative
/// f16s carry the sign bit (`>= 0x8000`), well above the NaN
/// threshold, so they're rejected too.
#[inline]
pub(crate) const fn opaque_f16_bits(bits: u16) -> bool {
    const ONE_MINUS_EPS_BITS: u16 = half::f16::from_f32_const(1.0 - EPS).to_bits();
    const NAN_EXP: u16 = 0x7C00;
    bits >= ONE_MINUS_EPS_BITS && bits < NAN_EXP
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
mod tests {
    use crate::primitives::approx::{
        EPS, approx_zero, canon_bits, hash_rect, hash_visual_f32, hash_visual_rect, noop_f32,
    };
    use crate::primitives::rect::Rect;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher as _;

    fn finish_hash(write: impl FnOnce(&mut DefaultHasher)) -> u64 {
        let mut hasher = DefaultHasher::new();
        write(&mut hasher);
        hasher.finish()
    }

    /// **The NaN audit.** Every *paint* no-op predicate — "would this
    /// put down a texel" — must answer `true` for a NaN anywhere in its
    /// inputs, because a NaN that survives the gate goes on to poison a
    /// bbox, a damage rect, or a shader lane, and does it silently.
    ///
    /// Deliberately excluded are the predicates that ask a *different*
    /// question: `approx_zero`, `Size::approx_zero`, `Rect::approx_zero`,
    /// `Corners::approx_zero`, and `TranslateScale::is_noop` all mean "is
    /// this value ≈ this constant", and they gate **fast paths**, not
    /// paint. Answering `true` there would route a NaN *into* the sharp /
    /// identity shortcut instead of away from it — the opposite of safe.
    /// They are covered by the shape-level gate instead, which drops a
    /// NaN before any of them is ever reached.
    #[test]
    fn every_paint_noop_predicate_treats_nan_as_invisible() {
        use crate::primitives::brush::{Brush, CurveBrush};
        use crate::primitives::color::{Color, ColorF16};
        use crate::primitives::mesh::Mesh;
        use crate::primitives::shadow::Shadow;
        use crate::primitives::size::Size;
        use crate::primitives::stroke::Stroke;
        use crate::scene::shapes::paint::ShapeStroke;
        use glam::Vec2;

        const N: f32 = f32::NAN;
        let nan_color = Color::rgba(0.0, 0.0, 0.0, N);
        let mut nan_mesh = Mesh::new();
        nan_mesh.vertex(Vec2::new(N, 0.0), Color::WHITE);
        nan_mesh.vertex(Vec2::ZERO, Color::WHITE);
        nan_mesh.vertex(Vec2::X, Color::WHITE);
        nan_mesh.triangle(0, 1, 2);

        let cases: &[(&str, bool)] = &[
            ("noop_f32", noop_f32(N)),
            ("Size::is_paint_empty/w", Size::new(N, 4.0).is_paint_empty()),
            ("Size::is_paint_empty/h", Size::new(4.0, N).is_paint_empty()),
            (
                "Rect::is_paint_empty/size",
                Rect::new(0.0, 0.0, N, 4.0).is_paint_empty(),
            ),
            (
                "Rect::is_paint_empty/min",
                Rect::new(N, 0.0, 4.0, 4.0).is_paint_empty(),
            ),
            ("Color::is_noop", nan_color.is_noop()),
            ("ColorF16::is_noop", ColorF16::from(nan_color).is_noop()),
            (
                "Stroke::is_noop/width",
                Stroke::solid(Color::WHITE, N).is_noop(),
            ),
            (
                "Stroke::is_noop/color",
                Stroke::solid(nan_color, 2.0).is_noop(),
            ),
            (
                "ShapeStroke::is_noop/width",
                ShapeStroke::from(Stroke::solid(Color::WHITE, N)).is_noop(),
            ),
            (
                "ShapeStroke::is_noop/color",
                ShapeStroke::from(Stroke::solid(nan_color, 2.0)).is_noop(),
            ),
            ("Brush::is_noop", Brush::Solid(nan_color).is_noop()),
            (
                "CurveBrush::is_noop",
                CurveBrush::Solid(nan_color).is_noop(),
            ),
            (
                "Shadow::is_noop/color",
                Shadow {
                    color: nan_color,
                    ..Shadow::default()
                }
                .is_noop(),
            ),
            (
                "Shadow::is_noop/blur",
                Shadow {
                    color: Color::WHITE,
                    blur: N,
                    ..Shadow::default()
                }
                .is_noop(),
            ),
            (
                "Shadow::is_noop/offset",
                Shadow {
                    color: Color::WHITE,
                    offset: Vec2::new(N, 0.0),
                    ..Shadow::default()
                }
                .is_noop(),
            ),
            (
                "Shadow::is_noop/spread",
                Shadow {
                    color: Color::WHITE,
                    spread: N,
                    ..Shadow::default()
                }
                .is_noop(),
            ),
            ("Mesh::is_noop", nan_mesh.is_noop()),
            // The convenience constructors pre-cache their own bbox
            // instead of routing through `Mesh::vertex`, so they need
            // covering separately — a bare fold there is how a NaN
            // vertex used to reach the shader with a finite box.
            (
                "Mesh::filled_triangle/is_noop",
                Mesh::filled_triangle(Vec2::new(N, 0.0), Vec2::ZERO, Vec2::X, Color::WHITE)
                    .is_noop(),
            ),
            (
                "Mesh::filled_polygon/is_noop",
                Mesh::filled_polygon(
                    &[Vec2::new(N, 0.0), Vec2::ZERO, Vec2::X, Vec2::Y],
                    Color::WHITE,
                )
                .is_noop(),
            ),
            // Chrome has no record-level gate to fall back on — it does
            // not pass through `Shapes::add` — so these four are the
            // only thing standing between a NaN `Background` and the
            // shader.
            (
                "ColorF16::is_noop/red",
                ColorF16::from(Color::rgba(N, 0.0, 0.0, 1.0)).is_noop(),
            ),
            (
                "Color::is_noop/red",
                Color::rgba(N, 0.0, 0.0, 1.0).is_noop(),
            ),
            (
                "LoweredShadow::is_noop/blur",
                crate::scene::shapes::paint::LoweredShadow::from(Shadow {
                    color: Color::WHITE,
                    blur: N,
                    ..Shadow::default()
                })
                .is_noop(),
            ),
            (
                "LoweredShadow::is_noop/offset",
                crate::scene::shapes::paint::LoweredShadow::from(Shadow {
                    color: Color::WHITE,
                    offset: Vec2::new(N, 0.0),
                    ..Shadow::default()
                })
                .is_noop(),
            ),
        ];

        let missed: Vec<&str> = cases
            .iter()
            .filter(|(_, is_noop)| !is_noop)
            .map(|(label, _)| *label)
            .collect();
        assert!(
            missed.is_empty(),
            "these paint no-op predicates let a NaN through: {missed:?}",
        );
    }

    #[test]
    fn approx_zero_handles_boundary_sign_and_nan() {
        let cases: &[(&str, f32, bool)] = &[
            ("exact_zero", 0.0, true),
            ("neg_zero", -0.0, true),
            ("at_eps", EPS, true),
            ("at_neg_eps", -EPS, true),
            ("just_above_eps", EPS * 1.1, false),
            ("just_below_neg_eps", -EPS * 1.1, false),
            ("nan", f32::NAN, false),
        ];
        for (label, v, want) in cases {
            assert_eq!(approx_zero(*v), *want, "case: {label}");
        }
    }

    #[test]
    fn exact_hash_helpers_collapse_only_signed_zero() {
        let positive = Rect::new(0.0, 0.0, 0.0, 0.0);
        let negative = Rect::new(-0.0, -0.0, -0.0, -0.0);
        let sub_eps = Rect::new(EPS * 0.5, 0.0, 0.0, 0.0);

        assert_eq!(
            finish_hash(|h| hash_rect(positive, h)),
            finish_hash(|h| hash_rect(negative, h)),
        );
        assert_ne!(
            finish_hash(|h| hash_rect(positive, h)),
            finish_hash(|h| hash_rect(sub_eps, h)),
        );
    }

    #[test]
    fn visual_hash_helpers_collapse_zero_noise_and_nan_payloads() {
        let zero = Rect::ZERO;
        let sub_eps = Rect::new(EPS * 0.5, -EPS * 0.5, EPS, -EPS);
        assert_eq!(
            finish_hash(|h| hash_visual_rect(zero, h)),
            finish_hash(|h| hash_visual_rect(sub_eps, h)),
        );

        let nan_a = f32::from_bits(0x7fc0_0001);
        let nan_b = f32::from_bits(0x7fc0_0002);
        assert_eq!(canon_bits(nan_a), canon_bits(nan_b));
        assert_eq!(
            finish_hash(|h| hash_visual_f32(nan_a, h)),
            finish_hash(|h| hash_visual_f32(nan_b, h)),
        );
    }
}
