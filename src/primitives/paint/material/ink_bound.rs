//! What holds a material's ink legible: the luminance bound an ink needs over a translucent fill,
//! as an 8-bit target stores the composite, the fallback alpha a theme's surface needs, and why a
//! bound cannot hold.

use crate::primitives::math::libm_ext::Libm;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::color::{RgbaF32, TEXT_CONTRAST};

/// The ceiling [`Material::with_max_luminance_for`](crate::Material::with_max_luminance_for)
/// derives: the highest relative luminance what shows under `fill` may reach for `ink` to hold
/// 4.5:1 on the composite, as an 8-bit target stores it; or why none can.
pub(super) fn ceiling_for(ink: RgbaF32, fill: RgbaF32) -> Result<f32, InkBound> {
    let alpha = fill.a;
    if !(0.0..1.0).contains(&alpha) {
        return Err(InkBound::FillNotTranslucent { alpha });
    }
    let ground = brightest_ground(ink)?;
    let fill_luminance = fill.luminance();
    // A fill of no alpha covers nothing, so the ink reads on what shows under it alone.
    if alpha > 0.0 && fill_luminance > ground {
        return Err(InkBound::InkFailsOnFill);
    }
    Ok((ground - alpha * fill_luminance) / (1.0 - alpha))
}

/// The floor [`Material::with_min_luminance_for`](crate::Material::with_min_luminance_for)
/// derives: the lowest relative luminance what shows under `fill` may reach for `ink` to hold
/// 4.5:1 on the composite, as an 8-bit target stores it, 0 where the fill alone clears it; or why
/// none can.
pub(super) fn floor_for(ink: RgbaF32, fill: RgbaF32) -> Result<f32, InkBound> {
    let alpha = fill.a;
    if !(0.0..1.0).contains(&alpha) {
        return Err(InkBound::FillNotTranslucent { alpha });
    }
    let ground = darkest_ground(ink)?;
    let fill_luminance = fill.luminance();
    // A fill of no alpha covers nothing, so the ink reads on what shows under it alone.
    if alpha > 0.0 && fill_luminance < ground {
        return Err(InkBound::InkFailsOnFill);
    }
    Ok(((ground - alpha * fill_luminance) / (1.0 - alpha)).max(0.0))
}

/// The brightest level a composite may reach and still hold `ink`, lighter than it, at 4.5:1 as an
/// 8-bit target stores it; or why no level is exact.
fn brightest_ground(ink: RgbaF32) -> Result<f32, InkBound> {
    let wanted = (ink.luminance() + 0.05) / TEXT_CONTRAST - 0.05;
    before_storing(wanted)
        .filter(|&ground| ground >= ceiling_bend())
        .ok_or(InkBound::InkTooDark { ground: wanted })
}

/// The darkest level a composite may reach and still hold `ink`, darker than it, at 4.5:1 as an
/// 8-bit target stores it; or why none can, past white.
fn darkest_ground(ink: RgbaF32) -> Result<f32, InkBound> {
    after_storing(TEXT_CONTRAST * (ink.luminance() + 0.05) - 0.05).ok_or(InkBound::InkFailsOnFill)
}

/// The least alpha at which `surface` holds `ink` at 4.5:1 over any page, as an 8-bit target
/// stores the composite: the fallback a material's box draws as while transparency is reduced.
/// What shows is the surface at `a` over the page, so a light ink is held over a white page,
/// `a·Y(surface) + (1 − a) ≤ c`, and a dark one over a black page, `a·Y(surface) ≥ f`, with `c`
/// and `f` the brightest and the darkest ground the ink holds on. Opaque where no alpha holds it:
/// the surface itself is then the most the fallback can do, as a float the material cannot hold
/// stays its opaque panel. The alpha rounds up to the 8-bit grid a theme file keeps a color on,
/// and up again where its f16 would read lower, so the composite never lands past the bound.
pub(crate) fn fallback_alpha(ink: RgbaF32, surface: RgbaF32) -> f32 {
    let surface_luminance = surface.luminance();
    let alpha = if ink.luminance() > surface_luminance {
        match brightest_ground(ink) {
            Ok(ceiling) if surface_luminance <= ceiling => {
                (1.0 - ceiling) / (1.0 - surface_luminance)
            }
            _ => 1.0,
        }
    } else {
        // Every darkest ground is at least `4.5 · 0.05 − 0.05 = 0.175`, so the division is safe.
        match darkest_ground(ink) {
            Ok(floor) if surface_luminance >= floor => floor / surface_luminance,
            _ => 1.0,
        }
    };
    grid_alpha_at_or_above(alpha.clamp(0.0, 1.0))
}

/// The least `k / 255` whose f16, as the GPU reads a color, is at least `alpha`, a fraction: the
/// 8-bit grid a theme file keeps a color on, rounded up so the alpha it stores never reads lower.
pub(super) fn grid_alpha_at_or_above(alpha: f32) -> f32 {
    debug_assert!(
        (0.0..=1.0).contains(&alpha),
        "an alpha is a fraction: {alpha}"
    );
    let mut step = (alpha * 255.0).ceil();
    // Ends by 255, whose f16 is 1.
    while half::f16::from_f32(step / 255.0).to_f32() < alpha {
        step += 1.0;
    }
    step / 255.0
}

/// Why a luminance bound cannot hold an ink at 4.5:1 over any backdrop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum InkBound {
    /// The fill is opaque or not a fraction: nothing shows through it to bound.
    FillNotTranslucent { alpha: f32 },
    /// The ground the ink needs lies where a colored pixel can store brighter than a neutral one.
    InkTooDark { ground: f32 },
    /// The ink misses its contrast on the fill made opaque, so no bound reaches it.
    InkFailsOnFill,
}

impl InkBound {
    /// Panic with the reason, as a caller that promised the bound holds.
    #[track_caller]
    pub(crate) fn refuse(self) -> ! {
        match self {
            Self::FillNotTranslucent { alpha } => {
                panic!("a fill's alpha is a fraction under 1: {alpha}")
            }
            Self::InkTooDark { ground } => panic!(
                "the ink is too dark for a ceiling: its ground {ground} lies where a colored pixel \
                 can store brighter than a neutral one"
            ),
            Self::InkFailsOnFill => panic!("the ink holds its contrast on the opaque fill"),
        }
    }
}

/// Past this luminance, `decode(encode(c) + step)` equals its concave envelope over `0..=1`, so a
/// ground there bounds what any mix of channels with its luminance stores, by Jensen on the
/// envelope, however close to 0 a channel of a saturated fill lies.
///
/// Near 0 the curve leaves the sRGB power segment, and its envelope there is the line from its
/// value at 0, `f₀ = step / 12.92`, tangent to the power segment, `(c^(1/2.4) + k)^2.4` with
/// `k = step / 1.055`. Tangency asks `k · (u + k)^1.4 = f₀` at `u = c^(1/2.4)`, so
/// `u + k = m`, [`bend_root`], and the bend is `(m − k)^2.4 ≈ 0.01292`. The floor's curve,
/// `decode(encode(c) − step)`, meets its convex envelope by the same tangency at `m^2.4 ≈
/// 0.01364`, under the 0.175 any floor's ground is at least, so a floor needs no such check.
pub(super) fn ceiling_bend() -> f32 {
    (bend_root() - STORED_STEP / 1.055).libm_powf(2.4)
}

/// `m = (1.055 / 12.92)^(1/1.4)`, the step cancelling out of [`ceiling_bend`]'s tangency.
pub(super) fn bend_root() -> f32 {
    (1.055_f32 / 12.92).libm_powf(1.0 / 1.4)
}

/// How far an 8-bit target stores a channel from its level, in encoded sRGB: half a step of the
/// dither the composite adds and half of the rounding to a byte.
const STORED_STEP: f32 = 1.0 / 255.0;

/// The brightest linear level a composite may reach and still store at most `g`: an 8-bit target
/// stores a channel within [`STORED_STEP`] of it, so the level is the one a step under `B`, the
/// highest byte that decodes to at most `g`. A colored pixel stores no brighter than a neutral one
/// of its luminance when that luminance is past [`ceiling_bend`]. `None` where no level lies a
/// step under `B`: a `g` under the first step above black, the ground of every ink darker than
/// `Y ≈ 0.175`.
fn before_storing(g: f32) -> Option<f32> {
    let level = |byte: u8| RgbaF32::from_srgba8(SrgbaU8::rgb(byte, byte, byte)).r;
    let mut top = RgbaF32::new(g, g, g, 1.0).to_srgba8().r;
    if level(top) > g {
        top = top.checked_sub(1)?;
    }
    Some(level(top.checked_sub(1)?))
}

/// The darkest linear level a composite may reach and still store at least `g`: the mirror of
/// [`before_storing`], a step above `B`, the lowest byte that decodes to at least `g`. A colored
/// pixel stores no darker than a neutral one of its luminance when that luminance is past the
/// floor's bend, which [`ceiling_bend`] derives. `None` where no level lies a step above `B`: a
/// `g` past the last step under white, the ground of every ink lighter than `Y ≈ 0.18`, which no
/// fill holds.
fn after_storing(g: f32) -> Option<f32> {
    let level = |byte: u8| RgbaF32::from_srgba8(SrgbaU8::rgb(byte, byte, byte)).r;
    let mut bottom = RgbaF32::new(g, g, g, 1.0).to_srgba8().r;
    if level(bottom) < g {
        bottom = bottom.checked_add(1)?;
    }
    Some(level(bottom.checked_add(1)?))
}
