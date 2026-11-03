//! The scaffold's colors and type ramp.

use palantir::{
    Chrome, ColorScheme, Corners, FontFamily, FontWeight, Metrics, Palette, RgbaF32, Stroke,
    TextStyle,
};

const M: Metrics = Metrics::DEFAULT;

/// A page's title: Fluent 2's Title 3, 24 px semibold, two steps over the 16 px subtitle
/// `Metrics` carries as its title (<https://fluent2.microsoft.design/typography>).
const PAGE_TITLE_PX: f32 = 24.0;

/// The longest line of prose, so a wide window does not stretch a paragraph past easy reading.
pub(crate) const MEASURE: f32 = 680.0;

/// The scaffold's colors this frame: roles of the palette the shown scheme wears, so a scheme
/// switch or an edit restyles the scaffold with palantir's widgets. A text style sets its ink
/// from here, as `TextStyle::stock` holds one scheme's ink whatever the window shows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tokens {
    pub(crate) ground: RgbaF32,
    pub(crate) surround: RgbaF32,
    pub(crate) raised: RgbaF32,
    /// Resting chrome for an interactive surface, and the track a control runs on.
    pub(crate) control: RgbaF32,
    /// The emphasis a control's hover and press reach for.
    pub(crate) control_strong: RgbaF32,
    pub(crate) border: RgbaF32,
    pub(crate) hairline: RgbaF32,
    pub(crate) ink: RgbaF32,
    pub(crate) ink_dim: RgbaF32,
    pub(crate) ink_faint: RgbaF32,
    pub(crate) accent: RgbaF32,
}

impl Tokens {
    pub(crate) const fn of(p: &Palette) -> Self {
        Self {
            ground: p.window_bg,
            surround: p.surround,
            raised: p.element,
            control: p.element_mid,
            control_strong: p.element_strong,
            border: p.border_mid(),
            hairline: p.border_soft(),
            ink: p.text,
            ink_dim: p.text_muted,
            ink_faint: p.text_disabled,
            accent: p.accent,
        }
    }

    pub(crate) const fn title(self) -> TextStyle {
        TextStyle::stock(ColorScheme::Dark)
            .with_font_size(PAGE_TITLE_PX)
            .with_weight(FontWeight::SEMI_BOLD)
            .with_color(self.ink)
    }

    /// A small-caps group label: the caption size, semibold, in the faint ink.
    pub(crate) const fn heading(self) -> TextStyle {
        TextStyle::stock(ColorScheme::Dark)
            .with_font_size(M.text.caption)
            .with_weight(FontWeight::SEMI_BOLD)
            .with_color(self.ink_faint)
    }

    pub(crate) const fn body(self) -> TextStyle {
        TextStyle::stock(ColorScheme::Dark)
            .with_font_size(M.text.body)
            .with_color(self.ink)
    }

    pub(crate) const fn dim(self) -> TextStyle {
        self.body().with_color(self.ink_dim)
    }

    pub(crate) const fn caption(self) -> TextStyle {
        TextStyle::stock(ColorScheme::Dark)
            .with_font_size(M.text.caption)
            .with_color(self.ink_dim)
    }

    pub(crate) const fn mono(self, color: RgbaF32) -> TextStyle {
        TextStyle::stock(ColorScheme::Dark)
            .with_family(FontFamily::MONO)
            .with_font_size(M.text.caption)
            .with_color(color)
    }

    /// A card's frame: the ground, the default edge, the card radius.
    pub(crate) fn card(self) -> Chrome {
        Chrome::rounded(self.ground, Corners::all(M.radius.card))
            .with_border(Stroke::new(self.border, M.stroke.hairline))
    }
}
