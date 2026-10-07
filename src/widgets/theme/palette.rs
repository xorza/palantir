//! The color roster every theme recipe draws from. [`Palette`] is the input to [`crate::Theme::from_palette`]; [`Palette::DEFAULT`] is the built-in neutral dark grayscale with a blue accent.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use glam::Vec2;

/// Semantic color roster for theme assembly. The three `element` rungs name a tier, never a widget state: a button rests on `element_mid` and hovers to `element_strong`, a menu row rests transparent and hovers to `element_mid`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Palette {
    /// Primary foreground / label ink.
    pub text: RgbaF32,
    /// De-emphasized foreground; also the base of the border ladder.
    pub text_muted: RgbaF32,
    /// Disabled-state foreground.
    pub text_disabled: RgbaF32,
    /// Window background (`Theme::window_clear`); also the active tab chip's fill.
    pub window_background: RgbaF32,
    /// Resting surface tier (disabled fills, menu panels).
    pub element: RgbaF32,
    /// One step brighter — resting chrome for interactive surfaces.
    pub element_mid: RgbaF32,
    /// Two steps brighter — the emphasis tier hover and press reach for.
    pub element_strong: RgbaF32,
    /// Focus-ring / pressed-stroke color.
    pub border_focused: RgbaF32,
    /// The accent (checked toggles, progress fill, selection wash).
    pub accent: RgbaF32,
}

impl Palette {
    /// Built-in neutral dark palette: the values `Theme::default` assembles from.
    pub const DEFAULT: Self = Self {
        text: RgbaF32::hex(0xffffff),
        text_muted: RgbaF32::hex(0xaaaaa8),
        text_disabled: RgbaF32::hex(0x878a8d),
        window_background: RgbaF32::hex(0x1a1a1a),
        element: RgbaF32::hex(0x343434),
        element_mid: RgbaF32::hex(0x3e3e3e),
        element_strong: RgbaF32::hex(0x4b4b4b),
        border_focused: RgbaF32::hex(0x105577),
        accent: RgbaF32::hex(0x9adbfb),
    };

    // Border ladder: TEXT_MUTED tints, since surface grays are too close to `element` to read as 1 px edges.
    /// The faintest edge — a rule, a divider.
    pub const fn border_soft(&self) -> RgbaF32 {
        self.text_muted.with_alpha(0.18)
    }

    /// The default edge — a panel or a field outline.
    pub const fn border_mid(&self) -> RgbaF32 {
        self.text_muted.with_alpha(0.22)
    }

    /// The loudest edge, for chrome that has to separate two lit surfaces.
    pub const fn border_strong(&self) -> RgbaF32 {
        self.text_muted.with_alpha(0.35)
    }

    /// Chrome for a body a [`crate::Popup`] drops from a trigger, one recipe so context menu, combo list and picker read as one system. Radius is the small-overlay step shared with [`TooltipTheme`](crate::TooltipTheme); the shadow separates it from what it opened over, since its fill matches the panels beneath.
    pub fn popup_panel(&self) -> Background {
        Background::rounded(self.element, Corners::all(4.0))
            .with_border(Stroke::new(self.border_mid(), 1.0))
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
