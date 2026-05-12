use crate::animation::animatable::Animatable;
use crate::primitives::color::{Color, Srgb8};
use tinyvec::ArrayVec;

/// Hard cap on stops in a single gradient. 8 covers >99% of UI use
/// (2-3 stops dominate, multi-stop bars rarely exceed 5). Hard-asserted
/// in `LinearGradient::new` — exceeding the cap is a caller bug, not a
/// silent truncation.
pub const MAX_STOPS: usize = 8;

/// GPU-wire form of a gradient's axis: direction vector + parametric
/// range. Mirrors WGSL's `@location(...) fill_axis: vec4<f32>`. The
/// shader does `t = (dot(local01, dir) - t0) / (t1 - t0)`, applies
/// `Spread`, then samples the LUT at `t`.
///
/// `repr(C)` so the field order maps to the four `f32` lanes the
/// vertex attribute reads. `Pod` for the cmd-buffer / `Quad` payload.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FillAxis {
    pub dir_x: f32,
    pub dir_y: f32,
    pub t0: f32,
    pub t1: f32,
}

impl FillAxis {
    /// All-zero axis used for solid quads. The shader ignores it when
    /// `FillKind::is_solid()`, so the value doesn't matter — keep it
    /// zeroed so Pod-byte cache keys are deterministic for solid
    /// quads.
    pub const ZERO: Self = Self {
        dir_x: 0.0,
        dir_y: 0.0,
        t0: 0.0,
        t1: 0.0,
    };
}

/// One colour stop in a gradient. `offset` is in 0..1 along the
/// gradient's parametric axis; out-of-range stops clamp at LUT bake.
/// `color` is stored as 8-bit sRGB (`Srgb8`) — gradient stops are
/// storage-only, never animated (snap on morph), and feed into a
/// u8 LUT bake, so 8-bit precision is sufficient and the 4× footprint
/// win matters when 8 stops × N gradients live in `Background.fill`.
#[derive(Copy, Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Stop {
    pub offset: f32,
    pub color: Srgb8,
}

impl Stop {
    /// Construct a stop. `offset` is clamped to 0..=1; out-of-range
    /// values clamp at construction rather than at bake time so the
    /// stored value matches what authors expect to round-trip.
    #[inline]
    pub fn new(offset: f32, color: impl Into<Srgb8>) -> Self {
        Self {
            offset: offset.clamp(0.0, 1.0),
            color: color.into(),
        }
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
    Pad = 0,
    /// Tile 0..1 across the surface.
    Repeat = 1,
    /// Tile mirrored.
    Reflect = 2,
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
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LinearGradient {
    pub angle: f32,
    pub stops: ArrayVec<[Stop; MAX_STOPS]>,
    pub spread: Spread,
    pub interp: Interp,
}

impl std::hash::Hash for LinearGradient {
    /// Hand-written: f32 fields (`angle`, per-stop `offset`) need
    /// canonical bit encoding so `-0.0` / `+0.0` and NaN bit patterns
    /// don't fragment cache keys. Drives `GradientCpuAtlas::register`
    /// row addressing.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(canon_bits(self.angle));
        state.write_u8(self.spread as u8);
        state.write_u8(self.interp as u8);
        state.write_u8(self.stops.len() as u8);
        for s in self.stops.iter() {
            state.write_u32(canon_bits(s.offset));
            s.color.hash(state);
        }
    }
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
            angle,
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

    /// Paints nothing visible when every stop is transparent.
    pub fn is_noop(&self) -> bool {
        self.stops.iter().all(|s| s.color.is_noop())
    }

    /// Gradient axis for the shader. `dir = (cos(angle), sin(angle))`;
    /// the shader projects each fragment's 0..1 object-local position
    /// onto `dir`, then maps the dot product through `(t0, t1)` to
    /// the LUT row.
    ///
    /// Slice 2 always emits `(t0, t1) = (0, 1)` and the raw
    /// `(cos, sin)` axis; diagonal gradients project to a sub-1.0
    /// range and rely on `Spread::Pad` to clamp. CSS-style
    /// corner-to-corner scaling is a slice 2.5 polish task.
    pub fn axis(&self) -> FillAxis {
        let (sin, cos) = self.angle.sin_cos();
        FillAxis {
            dir_x: cos,
            dir_y: sin,
            t0: 0.0,
            t1: 1.0,
        }
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
                g.hash(state);
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
    fn canon_bits_collapses_equivalent_f32_patterns() {
        let nan_a = f32::from_bits(0x7fc0_0001);
        let nan_b = f32::from_bits(0x7fc0_0002);
        assert!(nan_a.is_nan() && nan_b.is_nan());
        let solid = |r, a| {
            Brush::Solid(Color {
                r,
                g: 0.0,
                b: 0.0,
                a,
            })
        };
        let cases: &[(&str, Brush, Brush)] = &[
            (
                "neg_zero_eq_pos_zero",
                Brush::Solid(Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                }),
                Brush::Solid(Color {
                    r: -0.0,
                    g: -0.0,
                    b: -0.0,
                    a: -0.0,
                }),
            ),
            (
                "nan_bit_patterns_collapse",
                solid(nan_a, 1.0),
                solid(nan_b, 1.0),
            ),
        ];
        for (label, x, y) in cases {
            assert_eq!(h(*x), h(*y), "case: {label}");
        }
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

    /// `LinearGradient` is inline-stored on every `Brush::Linear`, so
    /// its size sets the floor for `Brush`, `Background.fill`,
    /// `Stroke.brush`, and every `Shape::*` variant carrying a brush.
    /// Pin the size so any silent footprint regression (added field,
    /// stop-cap bump) trips a test rather than diffusing through the
    /// codebase. The exact number below is a function of `MAX_STOPS = 8`
    /// + `repr(C)` field layout; recompute when those change.
    #[test]
    fn linear_gradient_size_is_76_bytes() {
        // 4 (angle) + 68 (ArrayVec<[Stop; 8]>: 8 × Stop + len) +
        // 1 (spread) + 1 (interp) + 2 (tail-pad to 4-byte alignment).
        // Each Stop is 8 B (4 offset + 4 Srgb8). Recompute if MAX_STOPS
        // or Stop layout changes.
        assert_eq!(std::mem::size_of::<LinearGradient>(), 76);
    }

    #[test]
    fn linear_two_stop_authoring() {
        let g = LinearGradient::two_stop(0.0, Color::hex(0x1a1a2e), Color::hex(0x16213e));
        assert_eq!(g.stops.len(), 2);
        assert_eq!(g.stops[0].offset, 0.0);
        assert_eq!(g.stops[1].offset, 1.0);
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
        assert_eq!(g.stops[1].offset, 0.5);
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
