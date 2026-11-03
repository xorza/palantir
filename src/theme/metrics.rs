//! The type, geometry and motion roster every theme recipe draws from, beside the [`Palette`](crate::Palette)'s colors. [`Metrics::DEFAULT`] is a 4 px grid on the Fluent 2 type ramp.

use crate::animation::animation_spec::AnimationSpec;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::backdrop_filter::{BackdropFilter, InkBound};
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use glam::Vec2;

/// The type, spacing, rounding, line, shadow, frost and transition steps of a theme, as [`Theme::from_parts`](crate::Theme::from_parts) reads them. Every font size, padding, gap, radius, border and shadow a recipe sets is one of these steps, so an app that passes its own scale moves every widget onto it; a widget's intrinsic sizes (a toggle's box, a scrollbar's thickness, a tab's width bounds) stay in its recipe.
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
    /// The frosted glass a surface over the others wears: a menu, a popup, the color picker's
    /// popup, a tooltip, a modal. `None` keeps them opaque, which a file that leaves it out
    /// reads. A float whose ink cannot hold 4.5:1 over any backdrop through the frost stays
    /// opaque, as it would read worse frosted; [`Theme::frost_floats`](crate::Theme::frost_floats)
    /// refuses one instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float_frost: Option<Frost>,
    /// What a control's state change runs under: a button's, a menu-bar title's, a menu row's and
    /// a tab chip's. A file that leaves it out reads [`AnimationSpec::SNAP`].
    #[serde(default, skip_serializing_if = "AnimationSpec::is_snap")]
    pub transition: AnimationSpec,
}

/// The font sizes, by prominence rather than by the surface that uses one, so a new surface picks
/// a tier by how loud its text is.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextSteps {
    /// Secondary text and dense surfaces: a tooltip, a caption over a group, a field's help.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::font_size")]
    pub caption: f32,
    /// The ambient text every widget reads: a button's label, a menu row, a field.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::font_size")]
    pub body: f32,
    /// A heading: a pane's or a dialog's title.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::font_size")]
    pub title: f32,
}

/// The spacing steps, smallest first.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpaceSteps {
    /// The densest rows: a subtitle under its title, a chip's vertical inset.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub xxs: f32,
    /// Inside a dense cluster: a control's vertical inset, a menu's padding, the gap between tabs.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub xs: f32,
    /// Between siblings: a field's horizontal inset, a row's vertical inset, a label's gap.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub sm: f32,
    /// A button's and a row's horizontal inset.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub md: f32,
    /// A dragged chip's distance below the pointer.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub lg: f32,
    /// A modal's padding and a menu row's shortcut gutter.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub xl: f32,
}

/// The corner radii, by the kind of surface.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RadiusSteps {
    /// A glyph-sized shape: a dock's drop preview.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub mark: f32,
    /// A field, a button, a menu row, a tooltip, a checkbox.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub control: f32,
    /// A tab chip and the ghost a dragged tab leaves.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub card: f32,
    /// A menu, a popup and a modal; over [`Self::control`], so a row's chip nests in the panel's corner.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub float: f32,
}

/// The line widths, by the kind of line.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StrokeSteps {
    /// A rule or a border, a field's included.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub hairline: f32,
    /// A field's caret, a drop preview, a small glyph's stroke.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub fine: f32,
    /// The focus ring, a tab's cap and caret, a check mark.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub line: f32,
}

/// A shadow's shape without its color: [`Palette::float_shadow`](crate::Palette::float_shadow)
/// casts it in the palette's shadow color.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShadowGeometry {
    /// Shift in logical pixels.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::offset2")]
    pub offset: Vec2,
    /// Gaussian σ in logical pixels, half CSS's `blur-radius`.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub blur: f32,
}

/// Frosted glass without its ink: [`BackdropFilter::frosted`] at [`Self::blur`] under the
/// surface's own fill at [`Self::tint_alpha`], its luminance bound derived from the dimmest ink
/// the surface paints.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Frost {
    /// The blur's Gaussian σ in logical pixels.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub blur: f32,
    /// The alpha of the surface's fill over the blur; a fraction, and 1 shows nothing through.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::fraction")]
    pub tint_alpha: f32,
}

impl Frost {
    /// `panel` frosted: its solid fill at [`Self::tint_alpha`] over the blur, bound so `ink`
    /// reads at 4.5:1 over any backdrop; or why not.
    pub(crate) fn dress(self, panel: &Chrome, ink: RgbaF32) -> Result<Chrome, FrostRefusal> {
        let fill = panel.fill.as_solid().ok_or(FrostRefusal::FillNotSolid)?;
        let tint = fill.with_alpha(self.tint_alpha);
        let filter = BackdropFilter::frosted(self.blur)
            .holding(ink, tint)
            .map_err(FrostRefusal::Ink)?;
        Ok(Chrome {
            fill: tint.into(),
            ..panel.clone()
        }
        .with_backdrop_filter(filter))
    }
}

/// Why [`Frost::dress`] left a surface as it was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FrostRefusal {
    /// The surface's fill is a gradient, with no one color to tint by.
    FillNotSolid,
    /// No luminance bound holds the ink through the frost.
    Ink(InkBound),
}

impl FrostRefusal {
    /// Panic with the reason, as a caller that promised the surface frosts.
    #[track_caller]
    pub(crate) fn refuse(self) -> ! {
        match self {
            Self::FillNotSolid => panic!("a floating surface to frost has a solid fill"),
            Self::Ink(bound) => bound.refuse(),
        }
    }
}

impl Metrics {
    /// `panel`, a surface over the others whose dimmest ink is `ink`, in [`Self::float_frost`];
    /// as it is without one, or where the frost cannot hold `ink`.
    pub(crate) fn frosted_float(&self, panel: Chrome, ink: RgbaF32) -> Chrome {
        match self.float_frost {
            Some(frost) => frost.dress(&panel, ink).unwrap_or(panel),
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
        // A tint at 184/255, about 0.72 and on the 8-bit grid a saved color keeps, over a σ 20
        // blur: wide enough that text under a float reads as color, not as text, as Fluent puts
        // acrylic under its flyouts
        // (https://learn.microsoft.com/windows/apps/develop/ui/in-app-acrylic).
        float_frost: Some(Frost {
            blur: 20.0,
            tint_alpha: 184.0 / 255.0,
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
