use crate::animation::animatable::Animatable;
use crate::primitives::color::{Color, Srgb8};
use tinyvec::ArrayVec;

/// Hard cap on stops in a single gradient. 8 covers >99% of UI use
/// (2-3 stops dominate, multi-stop bars rarely exceed 5). Hard-asserted
/// in `LinearGradient::new` — exceeding the cap is a caller bug, not a
/// silent truncation.
pub const MAX_STOPS: usize = 8;

/// One colour stop in a gradient. `offset` is in 0..1 along the
/// gradient's parametric axis; out-of-range stops clamp at LUT bake.
/// `color` is stored as 8-bit sRGB (`Srgb8`) — gradient stops are
/// storage-only, never animated (snap on morph), and feed into a
/// u8 LUT bake, so 8-bit precision is sufficient and the 4× footprint
/// win matters when 8 stops × N gradients live in `Background.fill`.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    bytemuck::Pod,
    bytemuck::Zeroable,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct Stop {
    pub offset_q: u32,
    pub color: Srgb8,
}

impl Stop {
    /// Construct a stop. `offset` is clamped to 0..=1 and quantized to
    /// `u32` bits so the struct stays `Pod` (no `f32` field — those
    /// don't compare bytewise after lerps with NaN/±0). The bake-time
    /// reader re-expands via `offset()`.
    #[inline]
    pub fn new(offset: f32, color: impl Into<Srgb8>) -> Self {
        Self {
            offset_q: offset.clamp(0.0, 1.0).to_bits(),
            color: color.into(),
        }
    }

    #[inline]
    pub fn offset(self) -> f32 {
        f32::from_bits(self.offset_q)
    }
}

/// How the gradient repeats outside the 0..1 parametric range.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Spread {
    /// Clamp to nearest edge stop. CSS default.
    #[default]
    Pad,
    /// Tile 0..1 across the surface.
    Repeat,
    /// Tile mirrored.
    Reflect,
}

/// Colour space the interpolation runs in. Affects the perceived
/// transition; doesn't change the stop colours themselves.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Interp {
    /// Perceptually uniform; matches CSS Color 4 default. Avoids the
    /// muddy midpoint of complementary-colour pairs (red↔green,
    /// blue↔orange).
    #[default]
    Oklab,
    /// Linear-RGB interpolation. Cheapest; what most rendering engines
    /// do by default. Visible midpoint dip on saturated complementary
    /// pairs.
    Linear,
    /// sRGB-space interpolation. Provided for compatibility with old
    /// design tools (Photoshop pre-2023, Figma).
    Srgb,
}

/// Linear gradient — paints colour along an axis at `angle` radians
/// (0 = →, π/2 = ↓). Object-space: gradient spans the brush owner's
/// bounding rect end-to-end at the given angle.
///
/// Stops live inline via `ArrayVec<[Stop; MAX_STOPS]>` so a
/// `LinearGradient` value is heap-free and `Copy`. Total size is ~80 B
/// (4 B angle + 64 B stops + 1 B spread + 1 B interp + pad).
#[derive(Clone, Copy, Debug, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct LinearGradient {
    pub angle_q: u32,
    pub stops: ArrayVec<[Stop; MAX_STOPS]>,
    pub spread: Spread,
    pub interp: Interp,
}

impl LinearGradient {
    /// General constructor. Asserts 2..=MAX_STOPS stops.
    pub fn new(angle: f32, stops: impl IntoIterator<Item = Stop>) -> Self {
        let mut sv: ArrayVec<[Stop; MAX_STOPS]> = ArrayVec::new();
        for s in stops {
            assert!(
                sv.len() < MAX_STOPS,
                "LinearGradient: stop count exceeds MAX_STOPS = {MAX_STOPS}",
            );
            sv.push(s);
        }
        assert!(
            sv.len() >= 2,
            "LinearGradient requires at least 2 stops, got {}",
            sv.len(),
        );
        Self {
            angle_q: angle.to_bits(),
            stops: sv,
            spread: Spread::default(),
            interp: Interp::default(),
        }
    }

    /// 2-stop shorthand — `c0` at offset 0, `c1` at offset 1. Covers
    /// the dominant UI-gradient pattern (panel chrome, button
    /// surfaces, headers).
    pub fn two_stop(angle: f32, c0: impl Into<Srgb8>, c1: impl Into<Srgb8>) -> Self {
        Self::new(angle, [Stop::new(0.0, c0), Stop::new(1.0, c1)])
    }

    /// 3-stop shorthand — `c0` at 0, `c1` at 0.5, `c2` at 1.
    pub fn three_stop(
        angle: f32,
        c0: impl Into<Srgb8>,
        c1: impl Into<Srgb8>,
        c2: impl Into<Srgb8>,
    ) -> Self {
        Self::new(
            angle,
            [Stop::new(0.0, c0), Stop::new(0.5, c1), Stop::new(1.0, c2)],
        )
    }

    pub fn with_spread(mut self, spread: Spread) -> Self {
        self.spread = spread;
        self
    }

    pub fn with_interp(mut self, interp: Interp) -> Self {
        self.interp = interp;
        self
    }

    #[inline]
    pub fn angle(&self) -> f32 {
        f32::from_bits(self.angle_q)
    }

    /// Paints nothing visible when every stop is transparent.
    pub fn is_noop(&self) -> bool {
        self.stops.iter().all(|s| s.color.is_noop())
    }
}

/// Paint source for fills and strokes.
///
/// `Solid(Color)` is the hot 99% path — 16 B inline, animation-lerpable.
/// `Linear(LinearGradient)` carries the gradient inline (~80 B);
/// gradient morph animations currently snap (see
/// `docs/roadmap/brushes.md` "Future work: gradient morph animation"),
/// and the rendering pipeline for non-solid variants lands in slice 2.
/// Until then, lowering sites call `as_solid().expect(...)` which
/// panics on `Linear` with a slice-2 TODO message.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Brush {
    Solid(Color),
    Linear(LinearGradient),
}

impl Brush {
    pub const TRANSPARENT: Self = Self::Solid(Color::TRANSPARENT);

    /// Paints nothing visible.
    #[inline]
    pub fn is_noop(&self) -> bool {
        match self {
            Brush::Solid(c) => c.is_noop(),
            Brush::Linear(g) => g.is_noop(),
        }
    }

    /// Extracts the underlying `Color` for the solid fast path. Returns
    /// `None` for gradient variants until slice 2's gradient rendering
    /// lands; downstream call sites currently `.expect()` with a
    /// slice-2 TODO message.
    #[inline]
    pub const fn as_solid(self) -> Option<Color> {
        match self {
            Brush::Solid(c) => Some(c),
            Brush::Linear(_) => None,
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
            Brush::Linear(g) => {
                state.write_u8(1);
                state.write_u32(canon_bits(g.angle()));
                state.write_u8(g.spread as u8);
                state.write_u8(g.interp as u8);
                state.write_u8(g.stops.len() as u8);
                for s in g.stops.iter() {
                    state.write_u32(canon_bits(s.offset()));
                    s.color.hash(state);
                }
            }
        }
    }
}

impl Animatable for Brush {
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        match (a, b) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(Color::lerp(x, y, t)),
            // Gradient morphs snap; see docs/roadmap/brushes.md
            // "Future work: gradient morph animation."
            _ => {
                if t >= 1.0 {
                    b
                } else {
                    a
                }
            }
        }
    }
    #[inline]
    fn sub(self, other: Self) -> Self {
        match (self, other) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(x.sub(y)),
            _ => self,
        }
    }
    #[inline]
    fn add(self, other: Self) -> Self {
        match (self, other) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(x.add(y)),
            _ => self,
        }
    }
    #[inline]
    fn scale(self, k: f32) -> Self {
        match self {
            Brush::Solid(c) => Brush::Solid(c.scale(k)),
            Brush::Linear(_) => self,
        }
    }
    #[inline]
    fn magnitude_squared(self) -> f32 {
        match self {
            Brush::Solid(c) => c.magnitude_squared(),
            Brush::Linear(_) => 0.0,
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
        assert_eq!(b.as_solid(), Some(c));
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

    #[test]
    fn linear_two_stop_authoring() {
        let g = LinearGradient::two_stop(0.0, Color::hex(0x1a1a2e), Color::hex(0x16213e));
        assert_eq!(g.stops.len(), 2);
        assert_eq!(g.stops[0].offset(), 0.0);
        assert_eq!(g.stops[1].offset(), 1.0);
        assert_eq!(g.spread, Spread::Pad);
        assert_eq!(g.interp, Interp::Oklab);
        assert!(!g.is_noop());
    }

    #[test]
    fn linear_three_stop_authoring() {
        let g = LinearGradient::three_stop(
            std::f32::consts::PI / 2.0,
            Color::hex(0x000000),
            Color::hex(0x808080),
            Color::hex(0xffffff),
        );
        assert_eq!(g.stops.len(), 3);
        assert_eq!(g.stops[1].offset(), 0.5);
    }

    #[test]
    fn linear_all_transparent_is_noop() {
        let g = LinearGradient::two_stop(0.0, Srgb8::TRANSPARENT, Srgb8::rgba(255, 255, 255, 0));
        assert!(g.is_noop());
        assert!(Brush::Linear(g).is_noop());
    }

    #[test]
    #[should_panic(expected = "exceeds MAX_STOPS")]
    fn linear_too_many_stops_panics() {
        let many: Vec<Stop> = (0..=MAX_STOPS)
            .map(|i| Stop::new(i as f32 / 8.0, Color::WHITE))
            .collect();
        let _ = LinearGradient::new(0.0, many);
    }

    #[test]
    #[should_panic(expected = "at least 2 stops")]
    fn linear_one_stop_panics() {
        let _ = LinearGradient::new(0.0, [Stop::new(0.0, Color::WHITE)]);
    }

    #[test]
    fn linear_brush_animatable_snaps_on_t_one() {
        let g0 = LinearGradient::two_stop(0.0, Color::BLACK, Color::WHITE);
        let g1 = LinearGradient::two_stop(0.0, Color::WHITE, Color::BLACK);
        let a = Brush::Linear(g0);
        let b = Brush::Linear(g1);
        // t < 1.0 snaps to a; t >= 1.0 snaps to b.
        assert_eq!(Brush::lerp(a, b, 0.5), a);
        assert_eq!(Brush::lerp(a, b, 1.0), b);
    }

    #[test]
    fn linear_brush_hash_stable_across_construction() {
        let g0 = LinearGradient::two_stop(0.5, Color::hex(0x336699), Color::hex(0xddaa44));
        let g1 = LinearGradient::two_stop(0.5, Color::hex(0x336699), Color::hex(0xddaa44));
        assert_eq!(h(Brush::Linear(g0)), h(Brush::Linear(g1)));
    }
}
