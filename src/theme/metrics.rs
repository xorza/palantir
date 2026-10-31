//! The type, geometry and motion roster every theme recipe draws from, beside the [`Palette`](crate::Palette)'s colors. [`Metrics::DEFAULT`] is a 4 px grid on the Fluent 2 type ramp.

use crate::animation::animation_spec::AnimationSpec;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use glam::Vec2;

/// The type, spacing, rounding, line, shadow and transition steps of a theme, as [`Theme::from_parts`](crate::Theme::from_parts) reads them. Every font size, padding, gap, radius, border and shadow a recipe sets is one of these steps, so an app that passes its own scale moves every widget onto it; a widget's intrinsic sizes (a toggle's box, a scrollbar's thickness, a tab's width bounds) stay in its recipe.
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
    /// The shadow a surface over the others casts: a menu, a popup, a tooltip, a modal.
    pub float_shadow: Shadow,
    /// What a control's state change runs under: a button's, a menu-bar title's, a menu row's and
    /// a tab chip's. `None` snaps, and a file that leaves it out reads `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<AnimationSpec>,
}

/// The font sizes, by prominence rather than by the surface that uses one, so a new surface picks
/// a tier by how loud its text is.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextSteps {
    /// Secondary text and dense surfaces: a tooltip, a caption over a group, a field's help.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub caption: f32,
    /// The ambient text every widget reads: a button's label, a menu row, a field.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub body: f32,
    /// A heading: a pane's or a dialog's title.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
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

impl Metrics {
    /// The inset of a row a pointer picks from: a menu item, a menu-bar title, a tab and the ghost
    /// a dragged tab leaves are one row.
    pub(crate) fn row_padding(&self) -> Spacing {
        Spacing::xy(self.space.md, self.space.sm)
    }

    /// A 4 px grid with a 2 px step under it, and the Fluent 2 type ramp (Caption 12, Body 14,
    /// Subtitle 16, <https://fluent2.microsoft.design/typography>): the values `Theme::default`
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
        // which its `MenuPopover` casts, in the half black a dark ground needs before a shadow reads.
        float_shadow: Shadow::drop(RgbaF32::BLACK.with_alpha8(128), Vec2::new(0.0, 8.0), 16.0),
        // Fluent's button transitions its fill, border and ink over `durationFaster`, 100 ms:
        // https://github.com/microsoft/fluentui/blob/master/packages/react-components/react-button/library/src/components/Button/useButtonStyles.styles.ts
        transition: Some(AnimationSpec::FAST),
    };
}

impl Default for Metrics {
    fn default() -> Self {
        Self::DEFAULT
    }
}
