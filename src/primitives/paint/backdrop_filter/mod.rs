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
//!   to hold the bound over white, which leaves a few percent of the backdrop to see. A floor,
//!   for dark ink on a light tint, waits for a workload.
//! - **Linear light.** The capture, the blur and the composite run in linear light, as the rest
//!   of the pipeline blends, so a blur keeps the energy of bright detail. Refused: blurring
//!   sRGB-encoded values to imitate a browser's look.
//! - **The default themes keep opaque popups and menus.** A filter costs nothing until an author
//!   asks for one. Refused: frosted defaults, which charge every app a capture under each open
//!   menu.
//!
//! [`RectShape::backdrop_filter`]: crate::widget::RectShape::backdrop_filter

use crate::primitives::math::domain;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use palantir_anim_derive::Animatable;

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
/// [length](crate::widget::domain::length), `saturation`, `noise` and a `max_luminance`
/// [factors](crate::widget::domain::factor), each a [lane](crate::widget::domain::lane) as it
/// packs into one, and the lens's own checks hold. Where it paints, a value a tween carried past
/// the lanes saturates at 65504, and the blur at σ 16384 physical px.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize, Animatable)]
#[serde(default)]
#[must_use]
pub struct BackdropFilter {
    /// The blur's Gaussian σ in logical px, as CSS `blur()` takes it; 0 leaves the pixels sharp.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub blur: f32,
    /// The color's saturation as CSS `saturate()` scales it: 1 leaves it, 0 is gray, above 1
    /// deepens it.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::factor")]
    pub saturation: f32,
    /// The amplitude of a grain fixed to the pane, so frosted glass does not read as a flat
    /// color; 0 is none, and a few hundredths show it.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::factor")]
    pub noise: f32,
    /// The highest relative luminance the pane shows under its tint, if any: a brighter pixel is
    /// scaled down to it, its hue and chroma kept. The lane it packs into rounds it down, never
    /// up; an 8-bit target then stores each channel within one encoded step of the composite,
    /// half a step of dither and half of the rounding to a byte.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::primitives::packed::serde::checked::optional_factor"
    )]
    #[animate(snap)]
    pub max_luminance: Option<f32>,
    /// Glass that bends what the pane shows across a rounded bezel at its edge, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[animate(snap)]
    pub lens: Option<Lens>,
}

/// Glass over a pane, refracting what it shows across a rounded bezel at its edge. The bezel
/// rises as a squircle `(1 − (1 − x)⁴)^¼` of the inward fraction `x`; a ray through it bends by
/// Snell's law, and a fixed top-left lamp glints off it.
///
/// Plain data: `bezel` and `thickness` are [lengths](crate::widget::domain::length), and
/// `refractive_index` is finite and at least 1 ([`Self::is_refractive_index`]).
/// `dispersion` and `highlight` are [fractions](crate::widget::domain::fraction), clamped
/// into `0..=1` where they paint.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
#[must_use]
pub struct Lens {
    /// The width of the rounded band at the edge, in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub bezel: f32,
    /// The glass's height inside the bezel, in logical px: how far it bends a ray aside.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub thickness: f32,
    /// The glass's index of refraction; 1 bends nothing, 1.5 is window glass.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::refractive_index")]
    pub refractive_index: f32,
    /// How far apart the index sits for red and blue, which fringes the bezel with color.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::fraction")]
    pub dispersion: f32,
    /// The strength of the lamp's glint off the bezel.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::fraction")]
    pub highlight: f32,
}

impl BackdropFilter {
    /// No pane: the pixels stay as they are.
    pub const NONE: Self = Self {
        blur: 0.0,
        saturation: 1.0,
        noise: 0.0,
        max_luminance: None,
        lens: None,
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
            lens: None,
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
    /// Unless `tint`'s alpha is a fraction under 1, and `ink` holds its contrast on `tint` made
    /// opaque.
    #[track_caller]
    pub fn with_max_luminance_for(self, ink: RgbaF32, tint: RgbaF32) -> Self {
        let alpha = tint.a;
        assert!(
            (0.0..1.0).contains(&alpha),
            "a tint's alpha is a fraction under 1: {alpha}"
        );
        debug_assert!(
            alpha * tint.r.min(tint.g).min(tint.b) > SRGB_TOE,
            "every channel of a ground clears the sRGB curve's linear toe, as `before_storing` needs"
        );
        let ground = before_storing((ink.luminance() + 0.05) / TEXT_CONTRAST - 0.05);
        let tint_luminance = tint.luminance();
        assert!(
            tint_luminance <= ground,
            "the ink holds its contrast on the opaque tint"
        );
        self.with_max_luminance((ground - alpha * tint_luminance) / (1.0 - alpha))
    }

    /// This filter seen through `lens`.
    pub const fn with_lens(mut self, lens: Lens) -> Self {
        self.lens = Some(lens);
        self
    }

    /// True when the filter changes nothing it shows, so a rect with it makes no pane. Takes
    /// `&self` because `Chrome` names it in a `skip_serializing_if`.
    pub const fn is_noop(&self) -> bool {
        self.blur == 0.0
            && self.saturation == 1.0
            && self.noise == 0.0
            && self.max_luminance.is_none()
            && self.lens.is_none()
    }

    /// Panics unless `blur` is a length, `saturation`, `noise` and a `max_luminance` factors, and
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
        if let Some(lens) = &self.lens {
            lens.validate();
        }
    }
}

impl Default for BackdropFilter {
    fn default() -> Self {
        Self::NONE
    }
}

impl NanCheck for BackdropFilter {
    fn has_nan(&self) -> bool {
        self.blur.is_nan()
            || self.saturation.is_nan()
            || self.noise.is_nan()
            || self.max_luminance.is_some_and(f32::is_nan)
            || self.lens.is_some_and(Lens::has_nan)
    }
}

impl Lens {
    pub(crate) const REFRACTIVE_INDEX_RULE: &str =
        "a refractive index must be finite and at least 1";

    /// True if `n` can be a refractive index: finite and at least 1, as no glass bends light
    /// away from the normal on entry.
    pub const fn is_refractive_index(n: f32) -> bool {
        n.is_finite() && n >= 1.0
    }

    /// Panics unless `bezel` and `thickness` are lengths and `refractive_index` a refractive
    /// index, each a [lane](domain::lane) as it packs into one.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        domain::lane(domain::length(self.bezel));
        domain::lane(domain::length(self.thickness));
        assert!(
            Self::is_refractive_index(self.refractive_index),
            "{}",
            Self::REFRACTIVE_INDEX_RULE
        );
        domain::lane(self.refractive_index);
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

/// A pill of window glass: a 24 px bezel, 16 px thick, of index 1.5, with a little color at the
/// rim and a soft glint.
impl Default for Lens {
    fn default() -> Self {
        Self {
            bezel: 24.0,
            thickness: 16.0,
            refractive_index: 1.5,
            dispersion: 0.06,
            highlight: 0.6,
        }
    }
}

/// The contrast WCAG 1.4.3 asks of text against its ground
/// (<https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum>).
const TEXT_CONTRAST: f32 = 4.5;

/// The linear level where the sRGB curve's linear toe ends and its power segment starts.
const SRGB_TOE: f32 = 0.003_130_8;

/// The brightest linear level a composite may reach and still store at most `g`: an 8-bit target
/// stores a channel within one encoded step of it, half a step of the dither the composite adds
/// and half of the rounding to a byte. So the level is the one a step under `B`, the highest byte
/// that decodes to at most `g`. A colored pixel stores no brighter than a neutral one of its
/// luminance: on the curve's power segment, where every channel of a ground lies as it keeps `α`
/// of the tint's, the stored level `decode(encode(c) + step)` is concave in `c`, so by Jensen the
/// weighted channels gain at most what their mean does.
fn before_storing(g: f32) -> f32 {
    let level = |byte: u8| RgbaF32::from_srgba(SrgbaU8::rgb(byte, byte, byte)).r;
    let mut top = RgbaF32::new(g, g, g, 1.0).to_srgba().r;
    if level(top) > g {
        top -= 1;
    }
    level(
        top.checked_sub(1)
            .expect("the ink holds its contrast a step above black"),
    )
}

#[cfg(test)]
mod tests;
