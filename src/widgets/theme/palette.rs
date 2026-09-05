//! The color roster every theme recipe draws from. [`Palette`] is the
//! public input to [`crate::Theme::from_palette`] — apps hand in their
//! own swatches and every widget recolors from one source instead of
//! re-deriving palantir's recipes per widget. [`Palette::DEFAULT`] is
//! the built-in neutral dark grayscale with a single blue accent.

use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use glam::Vec2;

/// Semantic color roster for theme assembly. Fields are the roles the
/// widget recipes key on; derived tints (the border ladder) and the one
/// chrome recipe overlays share live as methods so a palette swap moves
/// them automatically.
///
/// The three `elem` rungs name a tier and never a widget state, because
/// no one mapping holds: a standard button rests on `elem_mid` and
/// hovers to `elem_strong`, while a menu row rests transparent and
/// hovers to `elem_mid`. A name saying "hover" would be one rung out of
/// step for whichever widget disagreed.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Palette {
    /// Primary foreground / label ink.
    pub text: RgbaF32,
    /// De-emphasized foreground; also the base of the border ladder.
    pub text_muted: RgbaF32,
    /// Disabled-state foreground.
    pub text_disabled: RgbaF32,
    /// Window / editor background (`Theme::window_clear`).
    pub terminal_bg: RgbaF32,
    /// Resting surface tier (disabled fills, menu panels).
    pub elem: RgbaF32,
    /// One step brighter — resting chrome for interactive surfaces.
    pub elem_mid: RgbaF32,
    /// Two steps brighter — the emphasis tier hover and press reach for.
    pub elem_strong: RgbaF32,
    /// Focus-ring / pressed-stroke color.
    pub border_focused: RgbaF32,
    /// The accent (checked toggles, progress fill, selection wash).
    pub accent: RgbaF32,
}

impl Palette {
    /// Built-in neutral dark palette — the values `Theme::default`
    /// assembles from.
    pub const DEFAULT: Self = Self {
        text: RgbaF32::hex(0xffffff),
        text_muted: RgbaF32::hex(0xaaaaa8),
        text_disabled: RgbaF32::hex(0x878a8d),
        terminal_bg: RgbaF32::hex(0x1a1a1a),
        elem: RgbaF32::hex(0x343434),
        elem_mid: RgbaF32::hex(0x3e3e3e),
        elem_strong: RgbaF32::hex(0x4b4b4b),
        border_focused: RgbaF32::hex(0x105577),
        accent: RgbaF32::hex(0x9adbfb),
    };

    // The border ladder — TEXT_MUTED tints, not grays: raw surface
    // grays sit too close to `elem`/`elem_mid` to read as edges at
    // 1 px.
    pub fn border_soft(&self) -> RgbaF32 {
        self.text_muted.with_alpha(0.18)
    }

    pub fn border_mid(&self) -> RgbaF32 {
        self.text_muted.with_alpha(0.22)
    }

    pub fn border_strong(&self) -> RgbaF32 {
        self.text_muted.with_alpha(0.35)
    }

    /// Chrome for a body a [`crate::Popup`] drops from a trigger: a context
    /// menu, a combo's list, a colour chip's picker. One recipe, so the three
    /// read as one system wherever they open beside each other.
    ///
    /// Radius sits on the small-floating-overlay step shared with
    /// [`TooltipTheme`](crate::TooltipTheme), not the modal's 12 — the same
    /// corner that reads as "soft" on a dialog reads as a bubble on a stack
    /// of 26 px rows. The shadow is what separates the body from what it
    /// opened over: the fill is `elem`, the same surface tier as the panels
    /// and cards underneath, so a hairline alone leaves it looking glued
    /// down.
    pub fn popup_panel(&self) -> Background {
        Background::rounded(self.elem, Corners::all(4.0))
            .with_stroke(Stroke::solid(self.border_mid(), 1.0))
            .with_shadow(Shadow::drop(
                RgbaF32::new(0.0, 0.0, 0.0, 0.5),
                Vec2::new(0.0, 3.0),
                6.0,
            ))
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::DEFAULT
    }
}
