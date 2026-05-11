use crate::animation::animatable::Animatable;
use crate::primitives::color::Color;

/// Paint source for fills and strokes. Slice 1 only carries the
/// `Solid(Color)` variant; gradient and image variants land in later
/// slices. `Color` widens to `Brush` everywhere a fill or stroke colour
/// used to live (`Background.fill`, `Stroke.brush`, every coloured
/// `Shape` variant, `PolylineColors::Single`).
///
/// `From<Color>` is the call-site escape hatch: `fill: palette::ELEM.into()`
/// in struct-literal position, `Brush::Solid(c)` when explicit construction
/// reads better.
///
/// Hand-`Hash` so the discriminant byte participates and `f32`
/// components are canonicalized (-0.0 → +0.0, NaN → quiet canonical
/// bits) — gradient variants will carry raw `f32` axes/stops where the
/// canonicalization actually matters; doing it for `Solid` too keeps
/// the helper single-purpose.
///
/// Hand-`Animatable` so cross-variant pairs can snap once they exist.
/// For slice 1 only `Solid ↔ Solid` is possible, which delegates to
/// `Color`'s per-component lerp.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Brush {
    Solid(Color),
}

impl Brush {
    pub const TRANSPARENT: Self = Self::Solid(Color::TRANSPARENT);

    /// Paints nothing visible. Mirrors `Color::is_noop` for `Solid`;
    /// future gradient variants will return `true` only when every stop
    /// is transparent.
    #[inline]
    pub const fn is_noop(self) -> bool {
        match self {
            Brush::Solid(c) => c.is_noop(),
        }
    }

    /// Extracts the underlying `Color` for the solid fast path. The
    /// composer / encoder call this in slice 1 to keep their GPU types
    /// `Color`-shaped; slice 2 replaces these sites with brush-aware
    /// lowering. `expect` rather than `Option` because slice 1 callers
    /// have already type-narrowed via `matches!(brush, Brush::Solid(_))`
    /// or the variant only exists in `Solid` form.
    #[inline]
    pub const fn as_solid(self) -> Color {
        match self {
            Brush::Solid(c) => c,
        }
    }
}

impl Default for Brush {
    #[inline]
    fn default() -> Self {
        Brush::TRANSPARENT
    }
}

impl From<Color> for Brush {
    #[inline]
    fn from(c: Color) -> Self {
        Brush::Solid(c)
    }
}

/// Canonicalize an `f32` so equal values hash identically: collapse
/// `-0.0` to `+0.0` and every NaN to a single quiet-NaN bit pattern.
/// Used by `Hash for Brush`; gradient variants will reuse it for
/// per-axis / per-stop fields.
#[inline]
fn canon_bits(f: f32) -> u32 {
    if f.is_nan() {
        f32::NAN.to_bits()
    } else if f == 0.0 {
        0u32
    } else {
        f.to_bits()
    }
}

impl std::hash::Hash for Brush {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Brush::Solid(c) => {
                state.write_u8(0);
                state.write_u32(canon_bits(c.r));
                state.write_u32(canon_bits(c.g));
                state.write_u32(canon_bits(c.b));
                state.write_u32(canon_bits(c.a));
            }
        }
    }
}

impl Animatable for Brush {
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        match (a, b) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(Color::lerp(x, y, t)),
        }
    }
    #[inline]
    fn sub(self, other: Self) -> Self {
        match (self, other) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(x.sub(y)),
        }
    }
    #[inline]
    fn add(self, other: Self) -> Self {
        match (self, other) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(x.add(y)),
        }
    }
    #[inline]
    fn scale(self, k: f32) -> Self {
        match self {
            Brush::Solid(c) => Brush::Solid(c.scale(k)),
        }
    }
    #[inline]
    fn magnitude_squared(self) -> f32 {
        match self {
            Brush::Solid(c) => c.magnitude_squared(),
        }
    }
    #[inline]
    fn zero() -> Self {
        Brush::Solid(Color::zero())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn h(b: Brush) -> u64 {
        let mut s = DefaultHasher::new();
        b.hash(&mut s);
        s.finish()
    }

    #[test]
    fn negative_zero_hashes_same_as_positive_zero() {
        let pos = Brush::Solid(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });
        let neg = Brush::Solid(Color {
            r: -0.0,
            g: -0.0,
            b: -0.0,
            a: -0.0,
        });
        assert_eq!(h(pos), h(neg));
    }

    #[test]
    fn nan_bit_patterns_collapse_to_one_hash() {
        let a = f32::from_bits(0x7fc0_0001);
        let b = f32::from_bits(0x7fc0_0002);
        assert!(a.is_nan() && b.is_nan());
        let ba = Brush::Solid(Color {
            r: a,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        let bb = Brush::Solid(Color {
            r: b,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        assert_eq!(h(ba), h(bb));
    }

    #[test]
    fn from_color_round_trip() {
        let c = Color::WHITE;
        let b: Brush = c.into();
        assert_eq!(b, Brush::Solid(c));
        assert_eq!(b.as_solid(), c);
    }

    #[test]
    fn solid_solid_animatable_lerp_matches_color() {
        let a = Color::BLACK;
        let b = Color::WHITE;
        let mid_color = Color::lerp(a, b, 0.5);
        let mid_brush = Brush::lerp(Brush::Solid(a), Brush::Solid(b), 0.5);
        assert_eq!(mid_brush, Brush::Solid(mid_color));
    }

    #[test]
    fn solid_is_noop_iff_color_is_noop() {
        assert!(Brush::Solid(Color::TRANSPARENT).is_noop());
        assert!(!Brush::Solid(Color::BLACK).is_noop());
    }
}
