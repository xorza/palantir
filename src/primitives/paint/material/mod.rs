//! [`Material`]: a backdrop effect a box wears under its fill, and what holds its ink legible.
//!
//! # Decisions
//!
//! - **A material is a backdrop effect with its own parameters.** [`Material::new`] takes any
//!   backdrop effect's name and a value of its own parameter type, laid out inline as
//!   [`MaterialEffect`], so an app's backdrop effect dresses a chrome as a stock one does, as
//!   Flutter's `BackdropFilter` takes `ImageFilter.blur` or `ImageFilter.shader`, Android's
//!   `RenderEffect` a blur or a runtime shader, and Skia's `SkImageFilters` `Blur` or
//!   `RuntimeShader`. Frosted and glass are palantir's stock effects
//!   ([`EffectName::FROSTED`](crate::EffectName::FROSTED), `GLASS`), each with its own parameter
//!   type. Refused: one input type every material effect reads, which fit the stock effects alone;
//!   a shared allocation of the parameters, which every material built in a frame would pay; a
//!   built-in filter type beside app effects; a closed enum of looks, whose looks are no effects.
//! - **It is named `Material`**, the platform term: Apple's materials, Fluent's Acrylic and Mica
//!   materials. Refused: `BackdropFilter`, as it names an effect and no filter; `Pane`, a dock's
//!   region; `Backdrop`, the pixels under a shape.
//! - **A material is a shape's.** A [`MaterialPaint`](crate::MaterialPaint) makes a rect, a
//!   circle or a triangle show the pixels painted before it in paint order, through the effect,
//!   under its own fill, its border over both, as CSS pairs `backdrop-filter` with
//!   `background-color` over the border box; a node wears one as a shape of its own. Every
//!   earlier layer and material is behind it, so glass over glass is exact. Refused: a material as a `Brush` (WinUI's `AcrylicBrush`, SwiftUI's `Material`),
//!   which every text and stroke taking a brush would have to refuse; "glass cannot sample
//!   glass", as SwiftUI rules.
//! - **The capture blurs, the effect looks.** `backdrop_blur` is the σ of the capture's one
//!   downsampled blur, a Gaussian σ in logical px as [`Shadow::blur`](crate::Shadow) and CSS
//!   `blur()` take it, of the pixels under the box before the effect reads them, whatever the
//!   effect; [`Material::blur`] is then the frost at saturation 1 and no grain, which shows what
//!   it reads as it is, each channel floored at 0. One effect is worn at many σ, so σ is the material's, as Android chains a
//!   blur `RenderEffect` into a runtime shader's and Flutter composes `ImageFilter.blur` inside
//!   `ImageFilter.shader`. Refused: a blur inside each effect, a full-resolution loop per pixel; σ
//!   among the effect's parameters, which palantir must read to plan the capture before any effect
//!   runs; a stock effect for the blur alone, a program and its pipelines for what the frost at
//!   those parameters paints, and which a tween to a frost could not blend.
//! - **A material reads past its edge.** The blur is the exact convolution of what is painted
//!   around the box, so content glows in before it slides under a bar, and the effect reads as
//!   far past the box as its source declares ([`EffectSource::reach`](crate::EffectSource::reach)),
//!   the stock glass as far as its lens bends a ray. Refused: CSS's in-bounds
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
//! - **The default themes frost their floats.** The stock menus, popups, tooltips and modal wear
//!   `Metrics::float_material`, each its theme slot's `material` under its panel, as WinUI's
//!   flyouts wear acrylic.
//! - **Reduced transparency draws a material's box as a plain box.** While transparency is
//!   reduced, by the system's setting or `UiPreferences::transparency`, no capture, blur or effect
//!   runs: the box's fill and material give way to [`Material::fallback`], its border and shadow
//!   as they are, one draw of the plain box pipeline, as WinUI's acrylic turns into its
//!   `FallbackColor`, tint included
//!   (<https://learn.microsoft.com/windows/apps/design/style/acrylic>), and a CSS author restyles
//!   `background` under `prefers-reduced-transparency`. By default the fallback is the theme's
//!   surface, `Theme::material_fallback`, its float surface translucent at the least alpha that
//!   holds its text at 4.5:1 over any page ([`fallback_alpha`]), so a material reads as the theme
//!   with no color to pick per scheme and stays readable where its luminance bounds, which hold ink
//!   over a capture alone, no longer apply; a float's is its own panel at the least alpha that
//!   holds its own ink; a scrim names its own dim. A brush, so a gradient look keeps one. Refused:
//!   the fallback under the fill in the material's own draw, a stage branch, slot lanes and a stock
//!   no-op effect for what one plain box draws; two draws, whose antialiased edges composite
//!   twice; the effect over the window's ground, which drifted from the theme's colors and ran the
//!   effect each frame; a choice of the ground or nothing, which named no color; a transparent
//!   default, which leaves a box's ink unheld; a switch in the theme, as the choice is the
//!   person's or the system's, as CSS's `prefers-reduced-transparency` is.
//! - **A tween snaps the effect and blends the rest**, as a theme transition snaps a
//!   [`PressEffect`](crate::PressEffect): two materials over one effect blend their parameters
//!   lane by lane. An absent material blends through the target at rest ([`Material::at_rest`]),
//!   its parameters kept, as no effect names a neutral for them.

pub(crate) mod backdrop_fallback;
pub(crate) mod backdrop_reach;
pub(crate) mod frosted_params;
pub(crate) mod glass_params;
pub(crate) mod lens;
pub(crate) mod lens_optics;
pub(crate) mod material_effect;
pub(crate) mod material_paint;

use crate::primitives::effect::effect_kind::BackdropEffect;
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::math::animatable::Animatable;
use crate::primitives::math::domain;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::math::libm_ext::Libm;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::color::{RgbaF32, TEXT_CONTRAST};
use crate::primitives::paint::material::backdrop_fallback::BackdropFallback;
use crate::primitives::paint::material::frosted_params::FrostedParams;
use crate::primitives::paint::material::glass_params::GlassParams;
use crate::primitives::paint::material::lens::Lens;
use crate::primitives::paint::material::material_effect::MaterialEffect;

/// A backdrop effect a box wears under its fill: the pixels painted before it in paint order,
/// blurred by the capture's σ, shown through the effect, held within its luminance bounds, under
/// the box's fill, as CSS pairs `backdrop-filter` with `background-color`.
///
/// The theme form of a backdrop effect, plain data a theme file saves and a transition blends; an
/// [`EffectPaint`](crate::widget::EffectPaint) draws any effect at run time, with parameters of its
/// own type, images and a clock, and a material draws as one does, under palantir's stage.
///
/// A material reads past its edge: what is painted around it glows in as the blur reaches, and
/// materials painted before it show through it, so glass over glass is exact. It costs a capture
/// and a blur on each frame where something painted before it changes under it, and nothing on a
/// frame where nothing does.
///
/// Plain data, checked where it enters a shape or a theme: `backdrop_blur` is a
/// [length](crate::widget::domain::length), each luminance bound a
/// [factor](crate::widget::domain::factor), each a [lane](crate::widget::domain::lane) as it
/// packs into one, each parameter finite, and the fallback holds its own checks. The effect must
/// be loaded where the box records, a backdrop effect that binds no image and takes as many lanes
/// as the material holds, as an [`EffectPaint`](crate::widget::EffectPaint)'s effect must be
/// loaded.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[must_use]
pub struct Material {
    /// The backdrop effect that shows what the material reads, with its parameters.
    pub effect: MaterialEffect,
    /// The σ in logical px the pixels under the box are blurred by before the effect reads them,
    /// as CSS `blur()` takes it; 0 reads them sharp.
    #[serde(
        default,
        deserialize_with = "crate::primitives::math::domain::file::lane_length",
        skip_serializing_if = "is_sharp"
    )]
    pub backdrop_blur: f32,
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
    /// What the box paints in place of its fill while transparency is reduced, a plain box with
    /// no material then: the theme's surface by default.
    #[serde(default, skip_serializing_if = "BackdropFallback::is_theme_surface")]
    pub fallback: BackdropFallback,
}

impl Material {
    /// `effect` reading `params`, of its own parameter type, laid out as [`MaterialEffect`]:
    /// sharp, with no luminance bound, falling back to the theme's surface while transparency is
    /// reduced.
    ///
    /// # Panics
    ///
    /// At compile time, unless every field of `U` is f32 lanes and they fit
    /// [`MaterialEffect::MAX_LANES`].
    pub const fn new<U: EffectParams>(effect: EffectName<U, BackdropEffect>, params: U) -> Self {
        Self {
            effect: MaterialEffect::new(effect, params),
            backdrop_blur: 0.0,
            max_luminance: None,
            min_luminance: None,
            fallback: BackdropFallback::ThemeSurface,
        }
    }

    /// The pixels behind blurred by σ `sigma` logical px, as they are, each channel floored at 0,
    /// as CSS `backdrop-filter: blur()`: [`EffectName::FROSTED`] at saturation 1 and no grain.
    pub const fn blur(sigma: f32) -> Self {
        Self::new(
            EffectName::FROSTED,
            FrostedParams {
                saturation: 1.0,
                noise: 0.0,
            },
        )
        .with_backdrop_blur(sigma)
    }

    /// Frosted glass: the pixels behind blurred by σ `sigma` logical px, read through
    /// [`FrostedParams::DEFAULT`], so a fill over it reads as a material:
    /// [`EffectName::FROSTED`].
    pub const fn frosted(sigma: f32) -> Self {
        Self::new(EffectName::FROSTED, FrostedParams::DEFAULT).with_backdrop_blur(sigma)
    }

    /// Clear glass: the pixels behind, sharp, seen through `lens`, with no saturation or grain:
    /// [`EffectName::GLASS`]; [`Self::with_backdrop_blur`] frosts it.
    pub const fn glass(lens: Lens) -> Self {
        Self::new(EffectName::GLASS, GlassParams::new(lens))
    }

    /// This material with the pixels under the box blurred by σ `sigma` logical px before its
    /// effect reads them.
    pub const fn with_backdrop_blur(mut self, sigma: f32) -> Self {
        self.backdrop_blur = sigma;
        self
    }

    /// This material falling back to `fallback` while transparency is reduced, in place of the
    /// theme's surface: the box's fill then, with no material.
    pub fn with_fallback(mut self, fallback: impl Into<Brush>) -> Self {
        self.fallback = BackdropFallback::Brush(fallback.into());
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
    pub(crate) fn max_luminance_for(&self, ink: RgbaF32, fill: RgbaF32) -> Result<Self, InkBound> {
        Ok(self.clone().with_max_luminance(ceiling_for(ink, fill)?))
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
    pub(crate) fn min_luminance_for(&self, ink: RgbaF32, fill: RgbaF32) -> Result<Self, InkBound> {
        Ok(self.clone().with_min_luminance(floor_for(ink, fill)?))
    }

    /// The bound that holds `ink` at 4.5:1 over any backdrop under `fill`: a ceiling for ink
    /// lighter than the fill, a floor for ink darker, or why neither can.
    pub(crate) fn holding(&self, ink: RgbaF32, fill: RgbaF32) -> Result<Self, InkBound> {
        if ink.luminance() > fill.luminance() {
            self.max_luminance_for(ink, fill)
        } else {
            self.min_luminance_for(ink, fill)
        }
    }

    /// This material with no blur and no bound: what an absent material blends through toward
    /// it, as an absent chrome blends through `Chrome::NONE`. Its effect, its parameters and its
    /// fallback stay, as no effect names a neutral for its parameters.
    pub const fn at_rest(self) -> Self {
        Self {
            backdrop_blur: 0.0,
            max_luminance: None,
            min_luminance: None,
            ..self
        }
    }

    /// Whether `backdrop_blur` is a length, each luminance bound a factor, each a lane, each
    /// parameter finite and the fallback valid: [`Self::validate`]'s test.
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
            self.backdrop_blur,
            bound(self.max_luminance),
            bound(self.min_luminance),
        ]) & self.effect.is_valid()
            & self.fallback.is_valid()
    }

    /// Panics unless `backdrop_blur` is a length, each luminance bound a factor, each a lane, each
    /// parameter finite and the fallback valid.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        domain::lane(domain::length(self.backdrop_blur));
        if let Some(luminance) = self.max_luminance {
            domain::lane(domain::factor(luminance));
        }
        if let Some(luminance) = self.min_luminance {
            domain::lane(domain::factor(luminance));
        }
        assert!(
            self.effect.is_valid(),
            "a material's parameters must be finite"
        );
        self.fallback.validate();
    }

    /// Coerce each blended value into its kind, as a curve overshooting below a floor leaves
    /// it: the blur a length and the floor a factor. The parameters' kinds are their effect's,
    /// which it reads as it reads any.
    pub(crate) const fn coerce(&mut self) {
        self.backdrop_blur = domain::raised_to(self.backdrop_blur, 0.0);
        if let Some(luminance) = &mut self.min_luminance {
            *luminance = domain::raised_to(*luminance, 0.0);
        }
    }
}

/// The ceiling [`Material::with_max_luminance_for`] derives: the highest relative luminance what
/// shows under `fill` may reach for `ink` to hold 4.5:1 on the composite, as an 8-bit target
/// stores it; or why none can.
fn ceiling_for(ink: RgbaF32, fill: RgbaF32) -> Result<f32, InkBound> {
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

/// The floor [`Material::with_min_luminance_for`] derives: the lowest relative luminance what
/// shows under `fill` may reach for `ink` to hold 4.5:1 on the composite, as an 8-bit target
/// stores it, 0 where the fill alone clears it; or why none can.
fn floor_for(ink: RgbaF32, fill: RgbaF32) -> Result<f32, InkBound> {
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
fn grid_alpha_at_or_above(alpha: f32) -> f32 {
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

/// The blur and the fallback blend, the fallback as a [`BackdropFallback`] does, and the
/// parameters lane by lane where both ends name one effect; the effect switches at `t = 0.5`, its
/// parameters with it where the ends name two, as two effects do not blend. An absent floor
/// blends through 0, which lifts nothing; the ceiling switches at `t = 0.5`, as its neutral, no
/// bound, is no lane value. A blend is coerced into the material's domain, as its coercion where
/// it paints.
impl Lerp for Material {
    #[inline]
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        if t == 0.0 {
            return a.clone();
        }
        if t == 1.0 {
            return b.clone();
        }
        let snap = if t < 0.5 { a } else { b };
        let effect = if a.effect.blends_with(&b.effect) {
            a.effect.zip(&b.effect, |a, b| f32::lerp(&a, &b, t))
        } else {
            snap.effect
        };
        let mut blend = Self {
            effect,
            backdrop_blur: f32::lerp(&a.backdrop_blur, &b.backdrop_blur, t),
            max_luminance: snap.max_luminance,
            min_luminance: match (a.min_luminance, b.min_luminance) {
                (None, None) => None,
                (a, b) => Some(f32::lerp(&a.unwrap_or(0.0), &b.unwrap_or(0.0), t)),
            },
            fallback: BackdropFallback::lerp(&a.fallback, &b.fallback, t),
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
            return a.clone();
        }
        if t == 1.0 {
            return b.clone();
        }
        match (a, b) {
            (None, None) => None,
            (Some(a), Some(b)) => Some(Material::lerp(a, b, t)),
            (Some(a), None) => Some(Material::lerp(a, &a.clone().at_rest(), t)),
            (None, Some(b)) => Some(Material::lerp(&b.clone().at_rest(), b, t)),
        }
    }
}

/// The blur and each parameter lane spring; the rest takes the target at once, as a `lerp_only`
/// field does. An absent material springs through the other end at rest.
impl Animatable for Option<Material> {
    #[inline]
    fn sub(&self, other: &Self) -> Self {
        let (a, b) = match (self, other) {
            (None, None) => return None,
            (Some(a), Some(b)) => (a.clone(), b.clone()),
            (Some(a), None) => (a.clone(), a.clone().at_rest()),
            (None, Some(b)) => (b.clone().at_rest(), b.clone()),
        };
        Some(Material {
            backdrop_blur: a.backdrop_blur - b.backdrop_blur,
            effect: a.effect.zip(&b.effect, |a, b| a - b),
            ..a
        })
    }
    #[inline]
    fn add(&self, other: &Self) -> Self {
        let Some(offset) = other else {
            return self.clone();
        };
        let base = self.clone().unwrap_or_else(|| offset.clone().at_rest());
        Some(Material {
            backdrop_blur: base.backdrop_blur + offset.backdrop_blur,
            effect: base.effect.zip(&offset.effect, |a, b| a + b),
            ..base
        })
    }
    #[inline]
    fn scale(&self, k: f32) -> Self {
        self.clone().map(|m| Material {
            backdrop_blur: m.backdrop_blur * k,
            effect: m.effect.map(|lane| lane * k),
            ..m
        })
    }
    #[inline]
    fn magnitude_squared(&self) -> f32 {
        self.as_ref().map_or(0.0, |m| {
            m.backdrop_blur * m.backdrop_blur
                + m.effect
                    .params()
                    .iter()
                    .map(|lane| lane * lane)
                    .sum::<f32>()
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
    /// A retarget onto a material over another effect snaps the effect, as a tween does: the
    /// lanes of two effects mean different things, so the value takes the target's, and the
    /// velocity's lanes start from rest. The blur keeps springing.
    #[inline]
    fn normalize_for_spring(&mut self, target: &Self, velocity: &mut Self) {
        let (Some(value), Some(target)) = (self.as_mut(), target) else {
            return;
        };
        if value.effect.blends_with(&target.effect) {
            return;
        }
        value.effect = target.effect;
        if let Some(velocity) = velocity {
            velocity.effect = target.effect.map(|_| 0.0);
        }
    }
}

/// Whether a blur is 0, which a file leaves out.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's `skip_serializing_if` hands the field by reference"
)]
pub(crate) fn is_sharp(sigma: &f32) -> bool {
    *sigma == 0.0
}

impl NanCheck for Material {
    fn has_nan(&self) -> bool {
        self.backdrop_blur.is_nan()
            || self.max_luminance.is_some_and(f32::is_nan)
            || self.min_luminance.is_some_and(f32::is_nan)
            || self.effect.has_nan()
            || self.fallback.has_nan()
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
