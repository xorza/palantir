//! The type, geometry and motion roster every theme recipe draws from, beside the [`Palette`](crate::Palette)'s colors. [`Metrics::DEFAULT`] is a 4 px grid on the Fluent 2 type ramp.

use crate::animation::animation_spec::AnimationSpec;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::{InkBound, Material};
use glam::Vec2;

/// The type, spacing, rounding, line, shadow, float material and transition steps of a theme, as
/// [`Theme::from_parts`](crate::Theme::from_parts) reads them. Every font size, padding, gap,
/// radius, border and shadow a recipe sets is one of these steps, so an app that passes its own
/// scale moves every widget onto it; a widget's intrinsic sizes (a toggle's box, a scrollbar's
/// thickness, a tab's width bounds) stay in its recipe.
///
/// Kept beside the palette rather than in it, as egui keeps `Style::spacing` beside `Style::visuals`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Metrics {
    /// Font sizes, by how loud the text is.
    pub text: TextSteps,
    /// Paddings, gaps and margins.
    pub space: SpaceSteps,
    /// Corner radii, by what a surface is.
    pub radius: RadiusSteps,
    /// Line widths, by what a line is.
    pub stroke: StrokeSteps,
    /// Where the shadow a surface over the others casts falls, and how soft it is: a menu, a
    /// popup, a tooltip, a modal. Its color is the palette's
    /// [`shadow`](crate::Palette::shadow), as a light ground wants a fainter one.
    pub float_shadow: ShadowGeometry,
    /// The material a surface over the others wears: a menu, a popup, the color picker's popup,
    /// a tooltip, a modal. `None` keeps them opaque, which a file that leaves it out reads. A
    /// float whose ink cannot hold 4.5:1 over any backdrop through the material stays opaque, as
    /// it would read worse dressed; [`Theme::dress_floats`](crate::Theme::dress_floats) refuses
    /// one instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float_material: Option<FloatMaterial>,
    /// What a control's state change runs under: a button's, a menu-bar title's, a menu row's and
    /// a tab chip's. A file that leaves it out reads [`AnimationSpec::SNAP`].
    #[serde(default, skip_serializing_if = "AnimationSpec::is_instant")]
    pub transition: AnimationSpec,
}

/// The font sizes, by prominence rather than by the surface that uses one, so a new surface picks
/// a tier by how loud its text is.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextSteps {
    /// Secondary text and dense surfaces: a tooltip, a caption over a group, a field's help.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::font_size")]
    pub caption: f32,
    /// The ambient text every widget reads: a button's label, a menu row, a field.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::font_size")]
    pub body: f32,
    /// A heading: a pane's or a dialog's title.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::font_size")]
    pub title: f32,
}

/// The spacing steps, smallest first.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpaceSteps {
    /// The densest rows: a subtitle under its title, a chip's vertical inset.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub xxs: f32,
    /// Inside a dense cluster: a control's vertical inset, a menu's padding, the gap between tabs.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub xs: f32,
    /// Between siblings: a field's horizontal inset, a row's vertical inset, a label's gap.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub sm: f32,
    /// A button's and a row's horizontal inset.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub md: f32,
    /// A dragged chip's distance below the pointer.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub lg: f32,
    /// A modal's padding and a menu row's shortcut gutter.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub xl: f32,
}

/// The corner radii, by the kind of surface.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RadiusSteps {
    /// A glyph-sized shape: a dock's drop preview.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub mark: f32,
    /// A field, a button, a menu row, a tooltip, a checkbox.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub control: f32,
    /// A tab chip and the ghost a dragged tab leaves.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub card: f32,
    /// A menu, a popup and a modal; over [`Self::control`], so a row's chip nests in the panel's corner.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub float: f32,
}

/// The line widths, by the kind of line.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StrokeSteps {
    /// A rule or a border, a field's included.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub hairline: f32,
    /// A field's caret, a drop preview, a small glyph's stroke.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub fine: f32,
    /// The focus ring, a tab's cap and caret, a check mark.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub line: f32,
}

/// A shadow's shape without its color: [`Palette::float_shadow`](crate::Palette::float_shadow)
/// casts it in the palette's shadow color.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShadowGeometry {
    /// Shift in logical pixels.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::offset2")]
    pub offset: Vec2,
    /// Gaussian σ in logical pixels, half CSS's `blur-radius`.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub blur: f32,
}

/// A float's material without its ink: [`Self::material`], any [`Material`], under the surface's
/// own fill at [`Self::fill_alpha`], its luminance bound derived from the dimmest ink the surface
/// paints.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FloatMaterial {
    /// The material, as [`Material::blur`], [`Material::frosted`] or [`Material::glass`] makes
    /// it, or an app's effect. Its luminance bounds are replaced by the one each float's ink
    /// needs.
    pub material: Material,
    /// The alpha of the surface's fill over the material; a fraction, and 1 shows nothing through.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::fraction")]
    pub fill_alpha: f32,
}

impl FloatMaterial {
    /// `panel` in this material: its solid fill at [`Self::fill_alpha`] over [`Self::material`],
    /// bound so `ink`, the dimmest ink the surface paints, reads at 4.5:1 over any backdrop: a
    /// ceiling for ink lighter than the fill, a floor for ink darker.
    ///
    /// # Panics
    ///
    /// Unless `panel`'s fill is a solid color, the fill is a fraction under 1, `ink` holds 4.5:1
    /// on the fill made opaque, and an ink lighter than the fill is light enough for a ceiling,
    /// as [`Material::with_max_luminance_for`] and its floor twin require.
    #[track_caller]
    pub fn dress(self, panel: &Chrome, ink: RgbaF32) -> Chrome {
        self.try_dress(panel, ink)
            .unwrap_or_else(|refusal| refusal.refuse())
    }

    /// [`Self::dress`], or why it cannot.
    pub(crate) fn try_dress(self, panel: &Chrome, ink: RgbaF32) -> Result<Chrome, MaterialRefusal> {
        let fill = panel
            .paint
            .fill
            .as_solid()
            .ok_or(MaterialRefusal::FillNotSolid)?;
        let fill = fill.with_alpha(self.fill_alpha);
        let unbound = Material {
            max_luminance: None,
            min_luminance: None,
            ..self.material
        };
        let material = unbound.holding(ink, fill).map_err(MaterialRefusal::Ink)?;
        Ok(panel.clone().with_fill(fill).with_material(material))
    }
}

/// Why [`FloatMaterial::try_dress`] left a surface as it was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MaterialRefusal {
    /// The surface's fill is a gradient, with no one color to fill by.
    FillNotSolid,
    /// No luminance bound holds the ink through the frost.
    Ink(InkBound),
}

impl MaterialRefusal {
    /// Panic with the reason, as a caller that promised the surface takes the material.
    #[track_caller]
    pub(crate) fn refuse(self) -> ! {
        match self {
            Self::FillNotSolid => panic!("a floating surface to dress has a solid fill"),
            Self::Ink(bound) => bound.refuse(),
        }
    }
}

impl Metrics {
    /// `panel`, a surface over the others whose dimmest ink is `ink`, in
    /// [`Self::float_material`]; as it is without one, or where the material cannot hold `ink`.
    pub(crate) fn dress_float(&self, panel: Chrome, ink: RgbaF32) -> Chrome {
        match self.float_material {
            Some(material) => material.try_dress(&panel, ink).unwrap_or(panel),
            None => panel,
        }
    }

    /// The inset of a row a pointer picks from: a menu item, a menu-bar title, a tab and the ghost
    /// a dragged tab leaves are one row.
    pub(crate) fn row_padding(&self) -> Spacing {
        Spacing::xy(self.space.md, self.space.sm)
    }

    /// A 4 px grid with a 2 px step under it, and the Fluent 2 type ramp (Caption 12, Body 14,
    /// Subtitle 16, <https://fluent2.microsoft.design/typography>): the values `Theme::stock`
    /// assembles from.
    pub const DEFAULT: Self = Self {
        text: TextSteps {
            caption: 12.0,
            body: 14.0,
            title: 16.0,
        },
        space: SpaceSteps {
            xxs: 2.0,
            xs: 4.0,
            sm: 8.0,
            md: 12.0,
            lg: 16.0,
            xl: 24.0,
        },
        radius: RadiusSteps {
            mark: 2.0,
            control: 4.0,
            card: 6.0,
            float: 10.0,
        },
        stroke: StrokeSteps {
            hairline: 1.0,
            fine: 1.5,
            line: 2.0,
        },
        // Fluent's `shadow16`, 8 px down and 16 px of blur (https://fluent2.microsoft.design/elevation),
        // which its `MenuPopover` casts.
        float_shadow: ShadowGeometry {
            offset: Vec2::new(0.0, 8.0),
            blur: 16.0,
        },
        // Frosted glass under a fill at 184/255, about 0.72 and on the 8-bit grid a saved color
        // keeps, over a σ 20 blur: wide enough that text under a float reads as color, not as
        // text, as Fluent puts acrylic under its flyouts
        // (https://learn.microsoft.com/windows/apps/develop/ui/in-app-acrylic).
        float_material: Some(FloatMaterial {
            material: Material::frosted(20.0),
            fill_alpha: 184.0 / 255.0,
        }),
        // Fluent's button transitions its fill, border and ink over `durationFaster`, 100 ms:
        // https://github.com/microsoft/fluentui/blob/master/packages/react-components/react-button/library/src/components/Button/useButtonStyles.styles.ts
        transition: AnimationSpec::FAST,
    };
}

impl Default for Metrics {
    fn default() -> Self {
        Self::DEFAULT
    }
}
