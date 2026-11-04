//! What a pane does to the pixels painted before it.
//!
//! # Decisions
//!
//! - **A backdrop filter is a property of a rect.** [`RectShape::backdrop_filter`] and
//!   `Chrome::backdrop_filter` make the rect's interior show the pixels painted before it in
//!   paint order, filtered, under its own fill and border; the fill tints them, as CSS pairs
//!   `backdrop-filter` with `background-color`. Every earlier layer and pane is behind it, so
//!   glass over glass is exact. Refused: a separate backdrop shape, whose tint could not fuse with
//!   the pane; a filter as a `Brush` (WinUI's `AcrylicBrush`, SwiftUI's `Material`), which every
//!   text and stroke taking a brush would have to refuse; "glass cannot sample glass", as
//!   SwiftUI rules.
//! - **It is named `BackdropFilter`, as CSS names it.** `Backdrop` already names the area behind
//!   an overlay (`widget::Backdrop`, `Modal::backdrop`).
//! - **The pane reads past its edge.** The blur is the exact convolution of what is painted
//!   around the rect, so content glows in before it slides under a bar. Refused: CSS's in-bounds
//!   read with mirrored edges and Android's clamped edges, which paint invented content there.
//! - **Three looks, one type.** `blur(σ)`, `frosted(σ)` and `glass(lens)` are constructors of one
//!   plain value. Blur is a Gaussian σ in logical px, as [`Shadow::blur`](crate::Shadow) and CSS
//!   `blur()` take it. Frosted adds Apple's saturation of 1.8 and grain. Refused: Acrylic's
//!   exclusion blend, which its replications drop; a free light direction.
//! - **A luminance ceiling keeps text over a pane legible.** `max_luminance` caps the relative
//!   luminance of what the pane shows, after the blur, saturation and grain and before the tint,
//!   by scaling the color, so its hue and chroma hold. The ground under a pane's text is then at
//!   most `(1 − α)·max_luminance + α·tint`, whatever is painted behind it, and an author derives
//!   the ceiling from the contrast its inks need, as WCAG 1.4.3 measures it, or has
//!   `with_max_luminance_for` derive it from the ink and the tint. Fluent's acrylic
//!   serves the same need with a luminosity layer (`AcrylicBrush::TintLuminosityOpacity`,
//!   <https://learn.microsoft.com/windows/winui/api/microsoft.ui.xaml.media.acrylicbrush>).
//!   Refused: that luminosity blend and CSS `brightness()`, which compress or scale the dark
//!   range too and so show less of the backdrop for the same bound; a tint alone, opaque enough
//!   to hold the bound over white, which leaves a few percent of the backdrop to see.
//! - **A luminance floor keeps dark ink over light glass legible**, the ceiling's mirror:
//!   `min_luminance` lifts what the pane shows by a neutral, so a black backdrop reaches it too,
//!   and `with_min_luminance_for` derives it from the ink and the tint. Refused: scaling up, as
//!   the ceiling scales down, which cannot lift black.
//! - **Linear light.** The capture, the blur and the composite run in linear light, as the rest
//!   of the pipeline blends, so a blur keeps the energy of bright detail. Refused: blurring
//!   sRGB-encoded values to imitate a browser's look.
//! - **The default themes frost their floats, and reduced transparency paints every pane
//!   solid.** The stock menus, popups, tooltips and modal wear `Metrics::float_material`, as
//!   WinUI's flyouts wear acrylic. While transparency is reduced, by the system's setting or
//!   `UiPreferences::transparency`, a pane paints its `fallback` or its fill made opaque, and
//!   no capture or blur runs, as WinUI's acrylic falls back to its `FallbackColor` when
//!   transparency effects or Battery Saver turn it off
//!   (<https://learn.microsoft.com/windows/apps/design/style/acrylic>). Refused: opaque
//!   defaults, which made every app rebuild its themes to frost them and again to stop; a switch
//!   in the theme, as the choice is the person's or the system's, as CSS's
//!   `prefers-reduced-transparency` is.
//!
//! [`RectShape::backdrop_filter`]: crate::widget::RectShape::backdrop_filter

use crate::primitives::math::animatable::Animatable;
use crate::primitives::math::domain;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::color::{RgbaF32, TEXT_CONTRAST};

/// A pane's backdrop filter: the pixels painted before it in paint order, blurred, saturated and
/// grained, seen through an optional lens, under the pane's own fill, its tint, as CSS pairs
/// `backdrop-filter` with `background-color`. [`Self::NONE`] (also `Default`) leaves the pixels
/// as they are and makes no pane.
///
/// A pane reads past its edge: what is painted around it glows in as the blur reaches, and
/// panes painted before it show through it, so glass over glass is exact. It costs a capture and
/// a blur on each frame where something painted before it changes under it, and nothing on a
/// frame where nothing does.
///
/// Plain data, checked where it enters a shape or a [`Chrome`](crate::Chrome): `blur` is a
/// [length](crate::widget::domain::length), `saturation`, `noise` and each luminance bound
/// [factors](crate::widget::domain::factor), each a [lane](crate::widget::domain::lane) as it
/// packs into one, and the lens's own checks hold. Where it paints, a value a tween carried past
/// the lanes saturates at 65504, and the blur at σ 16384 physical px.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
#[must_use]
pub struct BackdropFilter {
    /// The blur's Gaussian σ in logical px, as CSS `blur()` takes it; 0 leaves the pixels sharp.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::lane_length")]
    pub blur: f32,
    /// The color's saturation as CSS `saturate()` scales it: 1 leaves it, 0 is gray, above 1
    /// deepens it.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::lane_factor")]
    pub saturation: f32,
    /// The amplitude of a grain fixed to the pane, so frosted glass does not read as a flat
    /// color; 0 is none, and a few hundredths show it.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::lane_factor")]
    pub noise: f32,
    /// The highest relative luminance the pane shows under its tint, if any: a brighter pixel is
    /// scaled down to it, its hue and chroma kept. The lane it packs into rounds it down, never
    /// up; an 8-bit target then stores each channel within one encoded step of the composite,
    /// half a step of dither and half of the rounding to a byte.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::primitives::math::domain::file::optional_lane_factor"
    )]
    pub max_luminance: Option<f32>,
    /// The lowest relative luminance the pane shows under its tint, if any: a darker pixel is
    /// lifted to it by a neutral, so a black backdrop reaches it too. The lane it packs into
    /// rounds it up, never down. Applied after [`Self::max_luminance`], so it wins where the two
    /// cross.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::primitives::math::domain::file::optional_lane_factor"
    )]
    pub min_luminance: Option<f32>,
    /// Glass that bends what the pane shows across a rounded bezel at its edge, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lens: Option<Lens>,
    /// The solid color the pane paints in place of the filter and its tint while transparency is
    /// reduced ([`Ui::is_transparency_reduced`](crate::Ui::is_transparency_reduced)), as WinUI's
    /// `AcrylicBrush::FallbackColor`; `None` paints the pane's fill made opaque, which suits a
    /// tinted pane and leaves a pane with no tint black, so glass with no tint names one. Stored
    /// as sRGB bytes, as a gradient stop stores its color, so it adds 8 bytes to a
    /// [`Chrome`](crate::Chrome) and to every look a widget animates, not 20; a file holds it as
    /// a color.
    #[serde(skip_serializing_if = "Option::is_none", with = "fallback_color")]
    pub fallback: Option<SrgbaU8>,
}

/// Glass over a pane, refracting what it shows across a rounded bezel at its edge. The bezel
/// rises as a squircle `(1 − (1 − x)⁴)^¼` of the inward fraction `x`; a ray through it bends by
/// Snell's law, and a fixed top-left lamp glints off it.
///
/// Plain data: `bezel` and `thickness` are [lengths](crate::widget::domain::length), and
/// `refractive_index` a [refractive index](crate::widget::domain::refractive_index).
/// `dispersion` and `highlight` are [fractions](crate::widget::domain::fraction), clamped
/// into `0..=1` where they paint.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default = "Lens::window_glass")]
#[must_use]
pub struct Lens {
    /// The width of the rounded band at the edge, in logical px.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub bezel: f32,
    /// The glass's height inside the bezel, in logical px: how far it bends a ray aside.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub thickness: f32,
    /// The glass's index of refraction; 1 bends nothing, 1.5 is window glass.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::refractive_index")]
    pub refractive_index: f32,
    /// The index's step from red to blue, `n_F − n_C` between Fraunhofer's C (656.3 nm) and F
    /// (486.1 nm) lines, as a glass catalog gives it; `refractive_index` is the index at the D
    /// line (589.3 nm). It spreads the spectrum across the bezel, fringing what the glass bends
    /// with color. Window glass (N-BK7) has 0.008, dense flint (N-SF11) 0.031; a UI's glass
    /// takes more, so the fringe shows.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::fraction")]
    pub dispersion: f32,
    /// The strength of the lamp's glint off the bezel.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::fraction")]
    pub highlight: f32,
}

impl BackdropFilter {
    /// No pane: the pixels stay as they are.
    pub const NONE: Self = Self {
        blur: 0.0,
        saturation: 1.0,
        noise: 0.0,
        max_luminance: None,
        min_luminance: None,
        lens: None,
        fallback: None,
    };

    /// The pixels behind blurred by σ `sigma` logical px, as CSS `backdrop-filter: blur()`.
    pub const fn blur(sigma: f32) -> Self {
        Self {
            blur: sigma,
            ..Self::NONE
        }
    }

    /// Frosted glass: the pixels behind blurred by σ `sigma` logical px, their color deepened
    /// by Apple's 1.8 and grained, so a tint over it reads as a material.
    pub const fn frosted(sigma: f32) -> Self {
        Self {
            blur: sigma,
            saturation: 1.8,
            noise: 0.04,
            max_luminance: None,
            min_luminance: None,
            lens: None,
            fallback: None,
        }
    }

    /// Clear glass: the pixels behind, sharp, seen through `lens`; [`Self::with_blur`] frosts it.
    pub const fn glass(lens: Lens) -> Self {
        Self {
            lens: Some(lens),
            ..Self::NONE
        }
    }

    /// This filter with σ `sigma` logical px of blur.
    pub const fn with_blur(mut self, sigma: f32) -> Self {
        self.blur = sigma;
        self
    }

    /// This filter with `saturation` as CSS `saturate()` takes it.
    pub const fn with_saturation(mut self, saturation: f32) -> Self {
        self.saturation = saturation;
        self
    }

    /// This filter with grain of amplitude `noise`.
    pub const fn with_noise(mut self, noise: f32) -> Self {
        self.noise = noise;
        self
    }

    /// This filter with what it shows held to at most `luminance`, a relative luminance.
    pub const fn with_max_luminance(mut self, luminance: f32) -> Self {
        self.max_luminance = Some(luminance);
        self
    }

    /// This filter with the [`max_luminance`](Self::max_luminance) that holds `ink`, the dimmest
    /// ink painted on the pane, at WCAG's 4.5:1 over any backdrop under `tint`, the pane's fill
    /// with its alpha, as an 8-bit target stores the composite. palantir derives it, as the bound
    /// depends on the dither its composite adds.
    ///
    /// The brightest ground `ink` holds 4.5:1 on is `g = (Y(ink) + 0.05) / 4.5 − 0.05`; a step of
    /// the 8-bit target lowers it to `g'`, what the composite may reach before the target stores
    /// it higher. The ground over a backdrop at the ceiling `c` is `(1 − α)·c + α·Y(tint)`, both
    /// in the linear light the composite runs in; so `c = (g' − α·Y(tint)) / (1 − α)`.
    ///
    /// # Panics
    ///
    /// Unless `tint`'s alpha is a fraction under 1, `ink` holds its contrast on `tint` made
    /// opaque, and `ink` is light enough that the ground it holds 4.5:1 on stores where the bound
    /// is exact: a luminance of about 0.237 or more, a grey of `#868686` or lighter.
    #[track_caller]
    pub fn with_max_luminance_for(self, ink: RgbaF32, tint: RgbaF32) -> Self {
        self.max_luminance_for(ink, tint)
            .unwrap_or_else(|refusal| refusal.refuse())
    }

    /// [`Self::with_max_luminance_for`], or why it cannot hold `ink`.
    pub(crate) fn max_luminance_for(self, ink: RgbaF32, tint: RgbaF32) -> Result<Self, InkBound> {
        let alpha = tint.a;
        if !(0.0..1.0).contains(&alpha) {
            return Err(InkBound::TintNotTranslucent { alpha });
        }
        let wanted = (ink.luminance() + 0.05) / TEXT_CONTRAST - 0.05;
        let Some(ground) = before_storing(wanted).filter(|&ground| ground >= ceiling_bend()) else {
            return Err(InkBound::InkTooDark { ground: wanted });
        };
        let tint_luminance = tint.luminance();
        if tint_luminance > ground {
            return Err(InkBound::InkFailsOnTint);
        }
        Ok(self.with_max_luminance((ground - alpha * tint_luminance) / (1.0 - alpha)))
    }

    /// This filter with what it shows held to at least `luminance`, a relative luminance.
    pub const fn with_min_luminance(mut self, luminance: f32) -> Self {
        self.min_luminance = Some(luminance);
        self
    }

    /// This filter with the [`min_luminance`](Self::min_luminance) that holds `ink`, the
    /// brightest ink painted on the pane, at WCAG's 4.5:1 over any backdrop under `tint`, as an
    /// 8-bit target stores the composite: the floor dark ink on light glass needs, as
    /// [`Self::with_max_luminance_for`] is the ceiling light ink on dark glass needs.
    ///
    /// The darkest ground `ink` holds 4.5:1 on is `g = 4.5 · (Y(ink) + 0.05) − 0.05`; a step of
    /// the 8-bit target raises it to `g'`, what the composite must reach for the target to store
    /// it no lower. The ground over a backdrop at the floor `f` is `(1 − α)·f + α·Y(tint)`, so
    /// `f = (g' − α·Y(tint)) / (1 − α)`, and 0 where the tint alone clears it.
    ///
    /// Any ink and any tint, saturated or not, take a floor: the darkest ground an ink holds 4.5:1
    /// on is at least `4.5 · 0.05 − 0.05 = 0.175`, past where the bound stops being exact.
    ///
    /// # Panics
    ///
    /// Unless `tint`'s alpha is a fraction under 1, and `ink` holds its contrast on `tint` made
    /// opaque.
    #[track_caller]
    pub fn with_min_luminance_for(self, ink: RgbaF32, tint: RgbaF32) -> Self {
        self.min_luminance_for(ink, tint)
            .unwrap_or_else(|refusal| refusal.refuse())
    }

    /// [`Self::with_min_luminance_for`], or why it cannot hold `ink`.
    pub(crate) fn min_luminance_for(self, ink: RgbaF32, tint: RgbaF32) -> Result<Self, InkBound> {
        let alpha = tint.a;
        if !(0.0..1.0).contains(&alpha) {
            return Err(InkBound::TintNotTranslucent { alpha });
        }
        let Some(ground) = after_storing(TEXT_CONTRAST * (ink.luminance() + 0.05) - 0.05) else {
            return Err(InkBound::InkFailsOnTint);
        };
        let tint_luminance = tint.luminance();
        if tint_luminance < ground {
            return Err(InkBound::InkFailsOnTint);
        }
        Ok(self.with_min_luminance(((ground - alpha * tint_luminance) / (1.0 - alpha)).max(0.0)))
    }

    /// The bound that holds `ink` at 4.5:1 over any backdrop under `tint`: a ceiling for ink
    /// lighter than the tint, a floor for ink darker, or why neither can.
    pub(crate) fn holding(self, ink: RgbaF32, tint: RgbaF32) -> Result<Self, InkBound> {
        if ink.luminance() > tint.luminance() {
            self.max_luminance_for(ink, tint)
        } else {
            self.min_luminance_for(ink, tint)
        }
    }

    /// This filter seen through `lens`.
    pub const fn with_lens(mut self, lens: Lens) -> Self {
        self.lens = Some(lens);
        self
    }

    /// This filter with `color` as the solid pane it paints while transparency is reduced; see
    /// [`Self::fallback`].
    pub const fn with_fallback(mut self, color: RgbaF32) -> Self {
        self.fallback = Some(color.to_srgba8());
        self
    }

    /// The solid fill a pane of fill `fill` paints in place of this filter while transparency
    /// is reduced: [`Self::fallback`], else `fill` made opaque.
    pub(crate) fn solid_infill(&self, fill: &Brush) -> Brush {
        self.fallback
            .map_or_else(|| fill.opaque(), |color| Brush::Solid(color.into()))
    }

    /// True when the filter changes nothing it shows, so a rect with it makes no pane. Takes
    /// `&self` because `Chrome` names it in a `skip_serializing_if`.
    pub const fn is_noop(&self) -> bool {
        self.blur == 0.0
            && self.saturation == 1.0
            && self.noise == 0.0
            && self.max_luminance.is_none()
            && self.min_luminance.is_none()
            && self.lens.is_none()
    }

    /// Panics unless `blur` is a length, `saturation`, `noise` and each luminance bound factors, and
    /// the lens valid.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        domain::lane(domain::length(self.blur));
        domain::lane(domain::factor(self.saturation));
        domain::lane(domain::factor(self.noise));
        if let Some(luminance) = self.max_luminance {
            domain::lane(domain::factor(luminance));
        }
        if let Some(luminance) = self.min_luminance {
            domain::lane(domain::factor(luminance));
        }
        if let Some(lens) = &self.lens {
            lens.validate();
        }
    }
}

/// [`BackdropFilter::fallback`] in a file: a color, held as the bytes it stores.
mod fallback_color {
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::srgba_u8::SrgbaU8;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[expect(
        clippy::ref_option,
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's `with` hands the field by reference, as it is stored"
    )]
    pub(super) fn serialize<S: Serializer>(
        color: &Option<SrgbaU8>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        color.map(RgbaF32::from).serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<SrgbaU8>, D::Error> {
        Ok(Option::<RgbaF32>::deserialize(deserializer)?.map(RgbaF32::to_srgba8))
    }
}

/// Why a luminance bound cannot hold an ink at 4.5:1 over any backdrop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum InkBound {
    /// The tint is opaque or not a fraction: nothing shows through it to bound.
    TintNotTranslucent { alpha: f32 },
    /// The ground the ink needs lies where a colored pixel can store brighter than a neutral one.
    InkTooDark { ground: f32 },
    /// The ink misses its contrast on the tint made opaque, so no bound reaches it.
    InkFailsOnTint,
}

impl InkBound {
    /// Panic with the reason, as a caller that promised the bound holds.
    #[track_caller]
    pub(crate) fn refuse(self) -> ! {
        match self {
            Self::TintNotTranslucent { alpha } => {
                panic!("a tint's alpha is a fraction under 1: {alpha}")
            }
            Self::InkTooDark { ground } => panic!(
                "the ink is too dark for a ceiling: its ground {ground} lies where a colored pixel \
                 can store brighter than a neutral one"
            ),
            Self::InkFailsOnTint => panic!("the ink holds its contrast on the opaque tint"),
        }
    }
}

impl Default for BackdropFilter {
    fn default() -> Self {
        Self::NONE
    }
}

/// `blur`, `saturation` and `noise` blend and spring. An absent floor or lens blends through its
/// neutral, as an absent chrome blends through `Chrome::NONE`: a floor of 0 lifts nothing, and
/// `Lens::FLAT` bends nothing. The ceiling switches at `t = 0.5`, as its neutral, no bound, is
/// no lane value. In motion the three take the target at once, as a `lerp_only` field does. A
/// blend is coerced into the filter's domain (`BackdropFilter::coerce`).
impl Lerp for BackdropFilter {
    #[inline]
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let (a, b) = (*a, *b);
        if t == 0.0 {
            return a;
        }
        if t == 1.0 {
            return b;
        }
        let mut blend = Self {
            blur: f32::lerp(&a.blur, &b.blur, t),
            saturation: f32::lerp(&a.saturation, &b.saturation, t),
            noise: f32::lerp(&a.noise, &b.noise, t),
            max_luminance: if t < 0.5 {
                a.max_luminance
            } else {
                b.max_luminance
            },
            min_luminance: match (a.min_luminance, b.min_luminance) {
                (None, None) => None,
                (a, b) => Some(f32::lerp(&a.unwrap_or(0.0), &b.unwrap_or(0.0), t)),
            },
            lens: match (a.lens, b.lens) {
                (None, None) => None,
                (a, b) => Some(Lens::lerp(
                    a.unwrap_or(Lens::FLAT),
                    b.unwrap_or(Lens::FLAT),
                    t,
                )),
            },
            fallback: if t < 0.5 { a.fallback } else { b.fallback },
        };
        blend.coerce();
        blend
    }
}

impl Animatable for BackdropFilter {
    #[inline]
    fn sub(&self, other: &Self) -> Self {
        Self {
            blur: self.blur - other.blur,
            saturation: self.saturation - other.saturation,
            noise: self.noise - other.noise,
            ..*self
        }
    }
    #[inline]
    fn add(&self, other: &Self) -> Self {
        Self {
            blur: self.blur + other.blur,
            saturation: self.saturation + other.saturation,
            noise: self.noise + other.noise,
            ..*self
        }
    }
    #[inline]
    fn scale(&self, k: f32) -> Self {
        Self {
            blur: self.blur * k,
            saturation: self.saturation * k,
            noise: self.noise * k,
            ..*self
        }
    }
    #[inline]
    fn magnitude_squared(&self) -> f32 {
        self.blur * self.blur + self.saturation * self.saturation + self.noise * self.noise
    }
    #[inline]
    fn zero() -> Self {
        Self {
            blur: 0.0,
            saturation: 0.0,
            noise: 0.0,
            max_luminance: None,
            min_luminance: None,
            lens: None,
            fallback: None,
        }
    }
    #[inline]
    fn from_motion(mut motion: Self) -> Self {
        motion.coerce();
        motion
    }
}

impl BackdropFilter {
    /// Coerce each blended value into its kind, as a curve overshooting below a floor leaves
    /// it: the blur a length, the saturation, noise and floor factors, the lens its own
    /// ([`Lens::coerce`]). CSS clamps an interpolated value into its property's range (CSS
    /// Values 4, "Range Checking").
    const fn coerce(&mut self) {
        self.blur = domain::raised_to(self.blur, 0.0);
        self.saturation = domain::raised_to(self.saturation, 0.0);
        self.noise = domain::raised_to(self.noise, 0.0);
        if let Some(luminance) = &mut self.min_luminance {
            *luminance = domain::raised_to(*luminance, 0.0);
        }
        if let Some(lens) = &mut self.lens {
            lens.coerce();
        }
    }
}

impl NanCheck for BackdropFilter {
    fn has_nan(&self) -> bool {
        self.blur.is_nan()
            || self.saturation.is_nan()
            || self.noise.is_nan()
            || self.max_luminance.is_some_and(f32::is_nan)
            || self.min_luminance.is_some_and(f32::is_nan)
            || self.lens.is_some_and(Lens::has_nan)
    }
}

impl Lens {
    /// A pill of window glass: a 24 px bezel, 16 px thick, of index 1.5, with a thin rainbow at
    /// the rim and a soft glint. What a theme file's lens holds where it leaves a field out.
    pub const WINDOW_GLASS: Self = Self {
        bezel: 24.0,
        thickness: 16.0,
        refractive_index: 1.5,
        dispersion: 0.06,
        highlight: 0.6,
    };

    /// [`Self::WINDOW_GLASS`], as serde's container default names a function.
    const fn window_glass() -> Self {
        Self::WINDOW_GLASS
    }

    /// Glass that bends nothing: no bezel, no thickness, index 1, no fringe and no glint. An
    /// absent lens blends through it.
    const FLAT: Self = Self {
        bezel: 0.0,
        thickness: 0.0,
        refractive_index: 1.0,
        dispersion: 0.0,
        highlight: 0.0,
    };

    /// Coerce the bezel and thickness to lengths and the index to at least 1.
    const fn coerce(&mut self) {
        self.bezel = domain::raised_to(self.bezel, 0.0);
        self.thickness = domain::raised_to(self.thickness, 0.0);
        self.refractive_index = domain::raised_to(self.refractive_index, 1.0);
    }

    /// Field by field, as `f32` blends.
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            bezel: f32::lerp(&a.bezel, &b.bezel, t),
            thickness: f32::lerp(&a.thickness, &b.thickness, t),
            refractive_index: f32::lerp(&a.refractive_index, &b.refractive_index, t),
            dispersion: f32::lerp(&a.dispersion, &b.dispersion, t),
            highlight: f32::lerp(&a.highlight, &b.highlight, t),
        }
    }

    /// Panics unless `bezel` and `thickness` are lengths and `refractive_index` a refractive
    /// index, each a [lane](domain::lane) as it packs into one.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        domain::lane(domain::length(self.bezel));
        domain::lane(domain::length(self.thickness));
        domain::lane(domain::refractive_index(self.refractive_index));
    }

    fn has_nan(self) -> bool {
        [
            self.bezel,
            self.thickness,
            self.refractive_index,
            self.dispersion,
            self.highlight,
        ]
        .iter()
        .any(|v| v.is_nan())
    }
}

/// Past this luminance, `decode(encode(c) + step)` equals its concave envelope over `0..=1`, so a
/// ground there bounds what any mix of channels with its luminance stores, by Jensen on the
/// envelope, however close to 0 a channel of a saturated tint lies.
///
/// Near 0 the curve leaves the sRGB power segment, and its envelope there is the line from its
/// value at 0, `f₀ = step / 12.92`, tangent to the power segment, `(c^(1/2.4) + k)^2.4` with
/// `k = step / 1.055`. Tangency asks `k · (u + k)^1.4 = f₀` at `u = c^(1/2.4)`, so
/// `u + k = m`, [`bend_root`], and the bend is `(m − k)^2.4 ≈ 0.01292`. The floor's curve,
/// `decode(encode(c) − step)`, meets its convex envelope by the same tangency at `m^2.4 ≈
/// 0.01364`, under the 0.175 any floor's ground is at least, so a floor needs no such check.
fn ceiling_bend() -> f32 {
    (bend_root() - STORED_STEP / 1.055).powf(2.4)
}

/// `m = (1.055 / 12.92)^(1/1.4)`, the step cancelling out of [`ceiling_bend`]'s tangency.
fn bend_root() -> f32 {
    (1.055_f32 / 12.92).powf(1.0 / 1.4)
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
/// tint holds.
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
