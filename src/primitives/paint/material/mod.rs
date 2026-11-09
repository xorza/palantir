//! [`Material`]: a backdrop effect a box wears under its fill, and what holds its ink legible.
//!
//! # Decisions
//!
//! - **A material is an effect over [`MaterialInput`].** It names a backdrop effect of fixed
//!   parameters, which palantir fills from it as it fills [`PressInput`](crate::PressInput) for a
//!   press. Blur, frosted and glass are palantir's stock effects
//!   ([`EffectName::BLUR`](crate::EffectName::BLUR), `FROSTED`, `GLASS`), and an app's backdrop
//!   effect over the same input dresses a chrome as a stock one does, as Flutter's
//!   `BackdropFilter` takes `ImageFilter.blur` or `ImageFilter.shader`, Android's
//!   `RenderEffect` a blur or a runtime shader, and Skia's `SkImageFilters` `Blur` or
//!   `RuntimeShader`. Refused: a built-in filter type beside app effects, two forms of one
//!   thing; a type-erased effect and its bytes in a `Chrome`, which no file holds and no tween
//!   blends; a closed enum of looks, whose looks are no effects.
//! - **It is named `Material`**, the platform term: Apple's materials, Fluent's Acrylic and Mica
//!   materials. Refused: `BackdropFilter`, as it names an effect and no filter; `Pane`, a dock's
//!   region; `Backdrop`, the pixels under a shape.
//! - **A material is a property of a box.** [`Paint::material`](crate::Paint::material) makes
//!   the box show the pixels painted before it in paint order, through the effect, under its own
//!   fill, its border over both, as CSS pairs `backdrop-filter` with `background-color` over the
//!   border box. Every earlier layer and material is behind it, so glass over glass is
//!   exact. Refused: a material as a `Brush` (WinUI's `AcrylicBrush`, SwiftUI's `Material`),
//!   which every text and stroke taking a brush would have to refuse; "glass cannot sample
//!   glass", as SwiftUI rules.
//! - **The capture blurs, the effect looks.** `blur` is the σ of the capture's one downsampled
//!   blur, a Gaussian σ in logical px as [`Shadow::blur`](crate::Shadow) and CSS `blur()` take
//!   it, whatever the effect; the stock blur is then the effect that shows what it reads. Refused:
//!   a blur inside each effect, a full-resolution loop per pixel.
//! - **A material reads past its edge.** The blur is the exact convolution of what is painted
//!   around the box, so content glows in before it slides under a bar. Refused: CSS's in-bounds
//!   read with mirrored edges and Android's clamped edges, which paint invented content there.
//! - **Palantir holds the ink legible, around any effect.** `max_luminance` caps the relative
//!   luminance of what the effect shows, before the fill, by scaling the color, so its hue and
//!   chroma hold. The ground under a box's text is then at most
//!   `(1 − α)·max_luminance + α·fill` whatever is painted behind it, and an author derives the
//!   ceiling from the contrast its inks need, as WCAG 1.4.3 measures it, or has
//!   [`Material::with_max_luminance_for`] derive it from the ink and the fill. Fluent's acrylic
//!   serves the same need with a luminosity layer (`AcrylicBrush::TintLuminosityOpacity`,
//!   <https://learn.microsoft.com/windows/winui/api/microsoft.ui.xaml.media.acrylicbrush>).
//!   `min_luminance` is its mirror for dark ink over light glass: it lifts what the effect shows
//!   by a neutral, so a black backdrop reaches it too. The bounds, the fill, the dither and the
//!   border are palantir's stage around the effect, so an app's effect keeps them too. Refused:
//!   the luminosity blend and CSS `brightness()`, which compress or scale the dark range too; a
//!   fill alone, opaque enough to hold the bound over white; the bounds in each effect.
//! - **Linear light.** The capture, the blur and the composite run in linear light, as the rest
//!   of the pipeline blends, so a blur keeps the energy of bright detail. Refused: blurring
//!   sRGB-encoded values to imitate a browser's look.
//! - **The default themes frost their floats, and reduced transparency reads the ground.** The
//!   stock menus, popups, tooltips and modal wear `Metrics::float_material`, as WinUI's flyouts
//!   wear acrylic. While transparency is reduced, by the system's setting or
//!   `UiPreferences::transparency`, no capture or blur runs, and a material does what its
//!   [`ReducedBackdrop`] says: by default its effect reads the window's ground, the clear color,
//!   so the box is the material over a bare window, opaque, exact for any effect, with no color
//!   for an author to pick in each scheme. Refused: the fill made opaque, which painted a light
//!   fill white and an absent fill black; a fallback color per use, as WinUI's
//!   `AcrylicBrush.FallbackColor` (<https://learn.microsoft.com/windows/apps/design/style/acrylic>),
//!   which each app picks per scheme; a switch in the theme, as the choice is the person's or the
//!   system's, as CSS's `prefers-reduced-transparency` is.
//! - **A tween snaps the effect and blends the rest**, as a theme transition snaps a
//!   [`PressEffect`](crate::PressEffect). An absent material blends through the target at rest
//!   ([`Material::at_rest`]).

pub(crate) mod lens;
pub(crate) mod lens_optics;
pub(crate) mod material_input;
pub(crate) mod reduced_backdrop;

use crate::primitives::effect::effect_kind::BackdropEffect;
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::math::animatable::Animatable;
use crate::primitives::math::domain;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::math::libm_ext::Libm;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::color::{RgbaF32, TEXT_CONTRAST};
use crate::primitives::paint::material::lens::Lens;
use crate::primitives::paint::material::material_input::MaterialInput;
use crate::primitives::paint::material::reduced_backdrop::ReducedBackdrop;

/// A backdrop effect a box wears under its fill: the pixels painted before it in paint order,
/// blurred by the capture's σ, shown through the effect, held within its luminance bounds, under
/// the box's fill, as CSS pairs `backdrop-filter` with `background-color`.
///
/// A material reads past its edge: what is painted around it glows in as the blur reaches, and
/// materials painted before it show through it, so glass over glass is exact. It costs a capture
/// and a blur on each frame where something painted before it changes under it, and nothing on a
/// frame where nothing does.
///
/// Plain data, checked where it enters a shape or a [`Chrome`](crate::Chrome): `blur` is a
/// [length](crate::widget::domain::length), each luminance bound a
/// [factor](crate::widget::domain::factor), each a [lane](crate::widget::domain::lane) as it
/// packs into one, and the input holds its own checks. The effect must be loaded where the box
/// records, as a shape's effect must.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[must_use]
pub struct Material {
    /// The backdrop effect that shows what the material reads.
    pub effect: EffectName<MaterialInput, BackdropEffect>,
    /// The capture's Gaussian σ in logical px, as CSS `blur()` takes it; 0 reads it sharp.
    #[serde(
        default,
        deserialize_with = "crate::primitives::math::domain::file::lane_length",
        skip_serializing_if = "is_sharp"
    )]
    pub blur: f32,
    /// What the effect reads besides, as palantir fills it.
    #[serde(default, skip_serializing_if = "MaterialInput::is_identity")]
    pub input: MaterialInput,
    /// The highest relative luminance the material shows under its fill, if any: a brighter pixel
    /// is scaled down to it, its hue and chroma kept. The lane it packs into rounds it down, never
    /// up; an 8-bit target then stores each channel within one encoded step of the composite,
    /// half a step of dither and half of the rounding to a byte.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::primitives::math::domain::file::optional_lane_factor"
    )]
    pub max_luminance: Option<f32>,
    /// The lowest relative luminance the material shows under its fill, if any: a darker pixel is
    /// lifted to it by a neutral, so a black backdrop reaches it too. The lane it packs into
    /// rounds it up, never down. Applied after [`Self::max_luminance`], so it wins where the two
    /// cross.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::primitives::math::domain::file::optional_lane_factor"
    )]
    pub min_luminance: Option<f32>,
    /// What the material paints while transparency is reduced; [`ReducedBackdrop::Ground`] by
    /// default.
    #[serde(default, skip_serializing_if = "ReducedBackdrop::is_ground")]
    pub reduced_transparency: ReducedBackdrop,
}

impl Material {
    /// The effect `effect` reading `input`, sharp, with no luminance bound.
    pub const fn new(
        effect: EffectName<MaterialInput, BackdropEffect>,
        input: MaterialInput,
    ) -> Self {
        Self {
            effect,
            blur: 0.0,
            input,
            max_luminance: None,
            min_luminance: None,
            reduced_transparency: ReducedBackdrop::Ground,
        }
    }

    /// The pixels behind blurred by σ `sigma` logical px, as CSS `backdrop-filter: blur()`:
    /// [`EffectName::BLUR`].
    pub const fn blur(sigma: f32) -> Self {
        Self::new(EffectName::BLUR, MaterialInput::IDENTITY).with_blur(sigma)
    }

    /// Frosted glass: the pixels behind blurred by σ `sigma` logical px, their color deepened
    /// by Apple's 1.8 and grained, so a fill over it reads as a material:
    /// [`EffectName::FROSTED`].
    pub const fn frosted(sigma: f32) -> Self {
        Self::new(
            EffectName::FROSTED,
            MaterialInput {
                saturation: 1.8,
                noise: 0.04,
                lens: Lens::NONE,
            },
        )
        .with_blur(sigma)
    }

    /// Clear glass: the pixels behind, sharp, seen through `lens`: [`EffectName::GLASS`];
    /// [`Self::with_blur`] frosts it.
    pub const fn glass(lens: Lens) -> Self {
        Self::new(
            EffectName::GLASS,
            MaterialInput {
                lens,
                ..MaterialInput::IDENTITY
            },
        )
    }

    /// This material with σ `sigma` logical px of blur.
    pub const fn with_blur(mut self, sigma: f32) -> Self {
        self.blur = sigma;
        self
    }

    /// This material with `saturation` as CSS `saturate()` takes it.
    pub const fn with_saturation(mut self, saturation: f32) -> Self {
        self.input.saturation = saturation;
        self
    }

    /// This material with grain of amplitude `noise`.
    pub const fn with_noise(mut self, noise: f32) -> Self {
        self.input.noise = noise;
        self
    }

    /// This material seen through `lens`.
    pub const fn with_lens(mut self, lens: Lens) -> Self {
        self.input.lens = lens;
        self
    }

    /// This material reading `backdrop` while transparency is reduced.
    pub const fn with_reduced_transparency(mut self, backdrop: ReducedBackdrop) -> Self {
        self.reduced_transparency = backdrop;
        self
    }

    /// This material with what it shows held to at most `luminance`, a relative luminance.
    pub const fn with_max_luminance(mut self, luminance: f32) -> Self {
        self.max_luminance = Some(luminance);
        self
    }

    /// This material with the [`max_luminance`](Self::max_luminance) that holds `ink`, the
    /// dimmest ink painted on the box, at WCAG's 4.5:1 over any backdrop under `fill`, the box's
    /// fill with its alpha, as an 8-bit target stores the composite. palantir derives it, as the
    /// bound depends on the dither its composite adds.
    ///
    /// The brightest ground `ink` holds 4.5:1 on is `g = (Y(ink) + 0.05) / 4.5 − 0.05`; a step of
    /// the 8-bit target lowers it to `g'`, what the composite may reach before the target stores
    /// it higher. The ground over a backdrop at the ceiling `c` is `(1 − α)·c + α·Y(fill)`, both
    /// in the linear light the composite runs in; so `c = (g' − α·Y(fill)) / (1 − α)`.
    ///
    /// # Panics
    ///
    /// Unless `fill`'s alpha is a fraction under 1, `ink` holds its contrast on `fill` made
    /// opaque, and `ink` is light enough that the ground it holds 4.5:1 on stores where the bound
    /// is exact: a luminance of about 0.237 or more, a grey of `#868686` or lighter.
    #[track_caller]
    pub fn with_max_luminance_for(self, ink: RgbaF32, fill: RgbaF32) -> Self {
        self.max_luminance_for(ink, fill)
            .unwrap_or_else(|refusal| refusal.refuse())
    }

    /// [`Self::with_max_luminance_for`], or why it cannot hold `ink`.
    pub(crate) fn max_luminance_for(self, ink: RgbaF32, fill: RgbaF32) -> Result<Self, InkBound> {
        let alpha = fill.a;
        if !(0.0..1.0).contains(&alpha) {
            return Err(InkBound::TintNotTranslucent { alpha });
        }
        let wanted = (ink.luminance() + 0.05) / TEXT_CONTRAST - 0.05;
        let Some(ground) = before_storing(wanted).filter(|&ground| ground >= ceiling_bend()) else {
            return Err(InkBound::InkTooDark { ground: wanted });
        };
        let fill_luminance = fill.luminance();
        if fill_luminance > ground {
            return Err(InkBound::InkFailsOnFill);
        }
        Ok(self.with_max_luminance((ground - alpha * fill_luminance) / (1.0 - alpha)))
    }

    /// This material with what it shows held to at least `luminance`, a relative luminance.
    pub const fn with_min_luminance(mut self, luminance: f32) -> Self {
        self.min_luminance = Some(luminance);
        self
    }

    /// This material with the [`min_luminance`](Self::min_luminance) that holds `ink`, the
    /// brightest ink painted on the box, at WCAG's 4.5:1 over any backdrop under `fill`, as an
    /// 8-bit target stores the composite: the floor dark ink on light glass needs, as
    /// [`Self::with_max_luminance_for`] is the ceiling light ink on dark glass needs.
    ///
    /// The darkest ground `ink` holds 4.5:1 on is `g = 4.5 · (Y(ink) + 0.05) − 0.05`; a step of
    /// the 8-bit target raises it to `g'`, what the composite must reach for the target to store
    /// it no lower. The ground over a backdrop at the floor `f` is `(1 − α)·f + α·Y(fill)`, so
    /// `f = (g' − α·Y(fill)) / (1 − α)`, and 0 where the fill alone clears it.
    ///
    /// Any ink and any fill, saturated or not, take a floor: the darkest ground an ink holds 4.5:1
    /// on is at least `4.5 · 0.05 − 0.05 = 0.175`, past where the bound stops being exact.
    ///
    /// # Panics
    ///
    /// Unless `fill`'s alpha is a fraction under 1, and `ink` holds its contrast on `fill` made
    /// opaque.
    #[track_caller]
    pub fn with_min_luminance_for(self, ink: RgbaF32, fill: RgbaF32) -> Self {
        self.min_luminance_for(ink, fill)
            .unwrap_or_else(|refusal| refusal.refuse())
    }

    /// [`Self::with_min_luminance_for`], or why it cannot hold `ink`.
    pub(crate) fn min_luminance_for(self, ink: RgbaF32, fill: RgbaF32) -> Result<Self, InkBound> {
        let alpha = fill.a;
        if !(0.0..1.0).contains(&alpha) {
            return Err(InkBound::TintNotTranslucent { alpha });
        }
        let Some(ground) = after_storing(TEXT_CONTRAST * (ink.luminance() + 0.05) - 0.05) else {
            return Err(InkBound::InkFailsOnFill);
        };
        let fill_luminance = fill.luminance();
        if fill_luminance < ground {
            return Err(InkBound::InkFailsOnFill);
        }
        Ok(self.with_min_luminance(((ground - alpha * fill_luminance) / (1.0 - alpha)).max(0.0)))
    }

    /// The bound that holds `ink` at 4.5:1 over any backdrop under `fill`: a ceiling for ink
    /// lighter than the fill, a floor for ink darker, or why neither can.
    pub(crate) fn holding(self, ink: RgbaF32, fill: RgbaF32) -> Result<Self, InkBound> {
        if ink.luminance() > fill.luminance() {
            self.max_luminance_for(ink, fill)
        } else {
            self.min_luminance_for(ink, fill)
        }
    }

    /// This material with no blur, its input at [`MaterialInput::IDENTITY`] and no bound: what an
    /// absent material blends through toward it, as an absent chrome blends through
    /// `Chrome::NONE`. Its effect and its `reduced_transparency` stay.
    pub const fn at_rest(self) -> Self {
        Self {
            blur: 0.0,
            input: MaterialInput::IDENTITY,
            max_luminance: None,
            min_luminance: None,
            ..self
        }
    }

    /// Whether `blur` is a length, each luminance bound a factor, each a lane, and the input
    /// valid: [`Self::validate`]'s test.
    #[inline]
    pub(crate) const fn is_valid(&self) -> bool {
        // An absent bound checks as 0, a factor; by hand, as `Option::unwrap_or` is not `const`.
        const fn bound(luminance: Option<f32>) -> f32 {
            match luminance {
                Some(luminance) => luminance,
                None => 0.0,
            }
        }
        domain::are_length_lanes([
            self.blur,
            bound(self.max_luminance),
            bound(self.min_luminance),
        ]) & self.input.is_valid()
    }

    /// Panics unless `blur` is a length, each luminance bound a factor, each a lane, and the
    /// input valid.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        domain::lane(domain::length(self.blur));
        if let Some(luminance) = self.max_luminance {
            domain::lane(domain::factor(luminance));
        }
        if let Some(luminance) = self.min_luminance {
            domain::lane(domain::factor(luminance));
        }
        self.input.validate();
    }

    /// Coerce each blended value into its kind, as a curve overshooting below a floor leaves
    /// it: the blur a length, the floor a factor, the input its own
    /// ([`MaterialInput::coerce`]).
    pub(crate) const fn coerce(&mut self) {
        self.blur = domain::raised_to(self.blur, 0.0);
        if let Some(luminance) = &mut self.min_luminance {
            *luminance = domain::raised_to(*luminance, 0.0);
        }
        self.input.coerce();
    }
}

/// Why a luminance bound cannot hold an ink at 4.5:1 over any backdrop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum InkBound {
    /// The fill is opaque or not a fraction: nothing shows through it to bound.
    TintNotTranslucent { alpha: f32 },
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
            Self::TintNotTranslucent { alpha } => {
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

/// `blur` and the input blend; the effect and `reduced_transparency` switch at `t = 0.5`, as two
/// effects do not blend. An absent floor blends through 0, which lifts nothing; the ceiling
/// switches at `t = 0.5`, as its neutral, no bound, is no lane value. A blend is coerced into the
/// material's domain, as its coercion where it paints.
impl Lerp for Material {
    #[inline]
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let (a, b) = (*a, *b);
        if t == 0.0 {
            return a;
        }
        if t == 1.0 {
            return b;
        }
        let snap = if t < 0.5 { a } else { b };
        let mut blend = Self {
            effect: snap.effect,
            blur: f32::lerp(&a.blur, &b.blur, t),
            input: MaterialInput::lerp(&a.input, &b.input, t),
            max_luminance: snap.max_luminance,
            min_luminance: match (a.min_luminance, b.min_luminance) {
                (None, None) => None,
                (a, b) => Some(f32::lerp(&a.unwrap_or(0.0), &b.unwrap_or(0.0), t)),
            },
            reduced_transparency: snap.reduced_transparency,
        };
        blend.coerce();
        blend
    }
}

/// An absent material blends through the other end at rest ([`Material::at_rest`]), as an absent
/// chrome blends through `Chrome::NONE`: a material that appears grows its blur from 0.
impl Lerp for Option<Material> {
    #[inline]
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        if t == 0.0 {
            return *a;
        }
        if t == 1.0 {
            return *b;
        }
        match (*a, *b) {
            (None, None) => None,
            (Some(a), Some(b)) => Some(Material::lerp(&a, &b, t)),
            (Some(a), None) => Some(Material::lerp(&a, &a.at_rest(), t)),
            (None, Some(b)) => Some(Material::lerp(&b.at_rest(), &b, t)),
        }
    }
}

/// `blur`, `saturation` and `noise` spring; the rest takes the target at once, as a `lerp_only`
/// field does. An absent material springs through the other end at rest.
impl Animatable for Option<Material> {
    #[inline]
    fn sub(&self, other: &Self) -> Self {
        let (a, b) = match (*self, *other) {
            (None, None) => return None,
            (Some(a), Some(b)) => (a, b),
            (Some(a), None) => (a, a.at_rest()),
            (None, Some(b)) => (b.at_rest(), b),
        };
        Some(Material {
            blur: a.blur - b.blur,
            input: MaterialInput {
                saturation: a.input.saturation - b.input.saturation,
                noise: a.input.noise - b.input.noise,
                ..a.input
            },
            ..a
        })
    }
    #[inline]
    fn add(&self, other: &Self) -> Self {
        let Some(offset) = *other else {
            return *self;
        };
        let base = self.unwrap_or_else(|| offset.at_rest());
        Some(Material {
            blur: base.blur + offset.blur,
            input: MaterialInput {
                saturation: base.input.saturation + offset.input.saturation,
                noise: base.input.noise + offset.input.noise,
                ..base.input
            },
            ..base
        })
    }
    #[inline]
    fn scale(&self, k: f32) -> Self {
        self.map(|m| Material {
            blur: m.blur * k,
            input: MaterialInput {
                saturation: m.input.saturation * k,
                noise: m.input.noise * k,
                ..m.input
            },
            ..m
        })
    }
    #[inline]
    fn magnitude_squared(&self) -> f32 {
        self.map_or(0.0, |m| {
            m.blur * m.blur
                + m.input.saturation * m.input.saturation
                + m.input.noise * m.input.noise
        })
    }
    #[inline]
    fn zero() -> Self {
        None
    }
    #[inline]
    fn from_motion(motion: Self) -> Self {
        motion.map(|mut m| {
            m.coerce();
            m
        })
    }
}

/// Whether a blur is 0, which a file leaves out.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's `skip_serializing_if` hands the field by reference"
)]
fn is_sharp(sigma: &f32) -> bool {
    *sigma == 0.0
}

impl NanCheck for Material {
    fn has_nan(&self) -> bool {
        self.blur.is_nan()
            || self.max_luminance.is_some_and(f32::is_nan)
            || self.min_luminance.is_some_and(f32::is_nan)
            || self.input.has_nan()
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
fn ceiling_bend() -> f32 {
    (bend_root() - STORED_STEP / 1.055).libm_powf(2.4)
}

/// `m = (1.055 / 12.92)^(1/1.4)`, the step cancelling out of [`ceiling_bend`]'s tangency.
fn bend_root() -> f32 {
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

#[cfg(test)]
mod tests;
