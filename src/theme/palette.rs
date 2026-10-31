//! The color roster every theme recipe draws from. [`Palette`] is the color input to [`crate::Theme::from_parts`]; [`Palette::DARK`] and [`Palette::LIGHT`] are the built-in looks, [`Palette::for_scheme`] picks one by [`ColorScheme`].

use crate::primitives::geometry::corners::Corners;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::metrics::Metrics;

/// Semantic color roster for theme assembly. The three `element` rungs name a tier, never a widget state: a button rests on `element_mid` and hovers to `element_strong`, a menu row rests transparent and hovers to `element_mid`. Each rung sits further from `window_bg` than the one before it: brighter in a dark palette, darker in a light one.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Palette {
    /// Primary foreground / label ink.
    pub text: RgbaF32,
    /// De-emphasized foreground; also the base of the border ladder.
    pub text_muted: RgbaF32,
    /// Disabled-state foreground.
    pub text_disabled: RgbaF32,
    /// Window background (`Theme::window_clear`); also the active tab chip's fill, so the chip
    /// joins the content under its strip.
    pub window_bg: RgbaF32,
    /// The band the window's controls sit on, a step off `window_bg`: a menu bar, a tab strip, and
    /// the ghost a dragged tab leaves.
    pub surround: RgbaF32,
    /// The raised and the recessed surface: a menu, a tooltip, a disabled control's fill.
    pub element: RgbaF32,
    /// One step further from `window_bg` — resting chrome for interactive surfaces.
    pub element_mid: RgbaF32,
    /// Two steps further — the emphasis tier hover and press reach for.
    pub element_strong: RgbaF32,
    /// Focus-ring / pressed-stroke color.
    pub border_focused: RgbaF32,
    /// The accent (checked toggles, progress fill, selection wash).
    pub accent: RgbaF32,
    /// Ink on an `accent` fill: a check mark, a radio's dot, an on switch's knob.
    pub on_accent: RgbaF32,
    /// The color a floating surface's shadow casts; [`Metrics::float_shadow`] holds its geometry.
    pub shadow: RgbaF32,
}

impl Palette {
    /// Built-in neutral dark palette: the values `Theme::default` assembles from.
    ///
    /// The semantic layer of Ayu Graphite, the palette darkroom paints with: content on
    /// `window_bg`, a ramp step under the `surround` band its menu bar and tab strips sit on, the
    /// three `element` rungs one neutral step apart, and ink off white, as Material's dark theme keeps
    /// text off `#fff`, where it vibrates against a dark ground
    /// (<https://m2.material.io/design/color/dark-theme.html>). `text_muted` is brighter than
    /// Ayu Graphite's `#a4a4a4`, as palantir rests a field on `element_mid` where Ayu Graphite rests
    /// it a rung lower: a placeholder there clears 4.5:1
    /// (<https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum>). `border_focused` is the
    /// accent: on a field's `element_mid` fill it reads 4.25:1, past the 3:1 WCAG 1.4.11 asks of a
    /// focus indicator (<https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast>).
    /// `on_accent` is the ground's own ink, as Material 3 paints a selected switch handle
    /// `on-primary`, dark on a light accent (<https://material-web.dev/components/switch>). The
    /// shadow is half black, what a dark ground needs before a shadow reads.
    pub const DARK: Self = Self {
        text: RgbaF32::hex(0xebebeb),
        text_muted: RgbaF32::hex(0xb2b2b2),
        text_disabled: RgbaF32::hex(0x797979),
        window_bg: RgbaF32::hex(0x141414),
        surround: RgbaF32::hex(0x1f1f1f),
        element: RgbaF32::hex(0x383838),
        element_mid: RgbaF32::hex(0x454545),
        element_strong: RgbaF32::hex(0x525252),
        border_focused: RgbaF32::hex(0x44b5ff),
        accent: RgbaF32::hex(0x44b5ff),
        on_accent: RgbaF32::hex(0x141414),
        shadow: RgbaF32::BLACK.with_alpha8(128),
    };

    /// Built-in light palette: Ayu Light's grounds and hues
    /// (<https://github.com/ayu-theme/ayu-colors/blob/master/themes/light.yaml>), moved until each
    /// pair a recipe paints clears its WCAG floor, as Ayu Graphite does for Ayu's dark.
    ///
    /// Ayu Light's own values miss them: its muted ink on its ground reads 3.15:1, its orange accent
    /// on white 2.28:1, its ground against its band 1.03:1. Here the ground is Ayu's `#fcfcfc`, the
    /// rungs step 1.10:1 apart in Ayu's cool grey, `element` is the white a raised menu or tooltip
    /// takes, set apart by its border and shadow, and the accent is Ayu's orange darkened at its own
    /// hue until white on it clears 4.5:1. The shadow is Fluent's light `shadow16` key, black at 14 %
    /// (<https://fluent2.microsoft.design/elevation>). `theme/tests/floors.rs` holds every pair.
    pub const LIGHT: Self = Self {
        text: RgbaF32::hex(0x3d4248),
        text_muted: RgbaF32::hex(0x5f6873),
        text_disabled: RgbaF32::hex(0x9aa1aa),
        window_bg: RgbaF32::hex(0xfcfcfc),
        surround: RgbaF32::hex(0xeff1f3),
        element: RgbaF32::hex(0xffffff),
        element_mid: RgbaF32::hex(0xe4e6e8),
        element_strong: RgbaF32::hex(0xd9dbdd),
        border_focused: RgbaF32::hex(0xa66509),
        accent: RgbaF32::hex(0xa66509),
        on_accent: RgbaF32::hex(0xffffff),
        shadow: RgbaF32::BLACK.with_alpha8(36),
    };

    /// The built-in palette of `scheme`.
    pub const fn for_scheme(scheme: ColorScheme) -> &'static Self {
        match scheme {
            ColorScheme::Light => &Self::LIGHT,
            ColorScheme::Dark => &Self::DARK,
        }
    }

    // Border ladder: TEXT_MUTED tints, since surface grays are too close to `element` to read as 1 px edges.
    /// The faintest edge — a rule, a divider.
    pub const fn border_soft(&self) -> RgbaF32 {
        self.text_muted.with_alpha8(46)
    }

    /// The default edge — a panel or a field outline.
    pub const fn border_mid(&self) -> RgbaF32 {
        self.text_muted.with_alpha8(56)
    }

    /// The loudest edge, for chrome that has to separate two lit surfaces.
    pub const fn border_strong(&self) -> RgbaF32 {
        self.text_muted.with_alpha8(89)
    }

    /// The shadow a surface over the others casts: this palette's [`Self::shadow`] at the
    /// geometry `m` gives it.
    pub const fn float_shadow(&self, m: &Metrics) -> Shadow {
        Shadow::drop(self.shadow, m.float_shadow.offset, m.float_shadow.blur)
    }

    /// Chrome for a body a [`crate::Popup`] drops from a trigger, one recipe so context menu, combo list and picker read as one system. It rounds and casts as every floating surface does; the shadow separates it from what it opened over, since its fill matches the panels beneath.
    pub fn popup_panel(&self, m: &Metrics) -> Chrome {
        Chrome::rounded(self.element, Corners::all(m.radius.float))
            .with_border(Stroke::new(self.border_mid(), m.stroke.hairline))
            .with_shadow(self.float_shadow(m))
    }
}
