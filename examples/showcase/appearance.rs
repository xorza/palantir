//! The tour's look a person sets: the accent, and the focus page's ring, over the stock palettes,
//! built into each window's themes once an edit.

use palantir::{
    ButtonTheme, ColorScheme, FocusRingTheme, Metrics, Okhsv, Palette, RgbaF32, Theme, Ui, WidgetId,
};

use crate::support;

const M: Metrics = Metrics::DEFAULT;

/// The blur of a frosted float, in logical px: Apple's regular material reads as about this
/// much, and the tint below keeps the text's bound from leaving little to see through.
const FROST_SIGMA: f32 = 20.0;
/// How much of a float's own fill covers the frosted backdrop.
const FROST_TINT: f32 = 0.6;

/// The accent seeds the rail offers beside the stock accent: Tailwind's 500 step of blue, teal,
/// green, orange, pink and violet (<https://tailwindcss.com/docs/colors>), each solved per scheme
/// as any seed is.
pub(crate) const SEEDS: [RgbaF32; 6] = [
    RgbaF32::hex(0x3b82f6),
    RgbaF32::hex(0x14b8a6),
    RgbaF32::hex(0x22c55e),
    RgbaF32::hex(0xf97316),
    RgbaF32::hex(0xec4899),
    RgbaF32::hex(0x8b5cf6),
];

/// The palettes the tour's themes are built from, the edits that made them, and whether each
/// window has built them yet.
#[derive(Debug)]
pub(crate) struct Appearance {
    /// The accent's seed; `None` is each palette's own accent.
    seed: Option<RgbaF32>,
    /// The focus page's ring: its color, `None` for the accent's, and its width.
    ring_color: Option<RgbaF32>,
    ring_width: f32,
    /// Whether menus, combo lists, the picker, tooltips and the modal wear frosted glass.
    frosted: bool,
    light: Palette,
    dark: Palette,
    /// Each seed of [`SEEDS`] solved for the light and the dark scheme, once, for the rail to show.
    light_seeds: [RgbaF32; SEEDS.len()],
    dark_seeds: [RgbaF32; SEEDS.len()],
    /// Moved by every edit; each window builds its themes once a generation, as each window's
    /// `Ui` holds its own.
    generation: u64,
}

impl Appearance {
    /// The stock look, not yet applied.
    pub(crate) fn new() -> Self {
        Self {
            seed: None,
            ring_color: None,
            ring_width: M.stroke.line,
            frosted: true,
            light: Palette::LIGHT,
            dark: Palette::DARK,
            light_seeds: SEEDS.map(|seed| accent_for(seed, ColorScheme::Light)),
            dark_seeds: SEEDS.map(|seed| accent_for(seed, ColorScheme::Dark)),
            generation: 1,
        }
    }

    pub(crate) const fn palette(&self, scheme: ColorScheme) -> &Palette {
        match scheme {
            ColorScheme::Light => &self.light,
            ColorScheme::Dark => &self.dark,
        }
    }

    pub(crate) const fn seed(&self) -> Option<RgbaF32> {
        self.seed
    }

    /// Each seed of [`SEEDS`] as `scheme` solves it.
    pub(crate) const fn seed_accents(&self, scheme: ColorScheme) -> &[RgbaF32; SEEDS.len()] {
        match scheme {
            ColorScheme::Light => &self.light_seeds,
            ColorScheme::Dark => &self.dark_seeds,
        }
    }

    /// Accent the tour with `seed`, or with each palette's own for `None`; a seed is solved per
    /// scheme by [`accent_for`].
    pub(crate) fn set_seed(&mut self, seed: Option<RgbaF32>) {
        if seed == self.seed {
            return;
        }
        self.seed = seed;
        for scheme in [ColorScheme::Light, ColorScheme::Dark] {
            let stock = Palette::for_scheme(scheme);
            let accent = seed.map_or(stock.accent, |seed| accent_for(seed, scheme));
            let palette = match scheme {
                ColorScheme::Light => &mut self.light,
                ColorScheme::Dark => &mut self.dark,
            };
            palette.accent = accent;
            palette.border_focused = accent;
        }
        self.generation += 1;
    }

    /// The ring the shown scheme draws: its color, set or the accent's, and its width.
    pub(crate) fn ring(&self, scheme: ColorScheme) -> FocusRingTheme {
        FocusRingTheme {
            color: self
                .ring_color
                .unwrap_or_else(|| self.palette(scheme).accent),
            width: self.ring_width,
        }
    }

    /// Draw the ring in `color` in either scheme, whatever the accent.
    pub(crate) fn set_ring_color(&mut self, color: RgbaF32) {
        if self.ring_color != Some(color) {
            self.ring_color = Some(color);
            self.generation += 1;
        }
    }

    pub(crate) fn set_ring_width(&mut self, width: f32) {
        if self.ring_width != width {
            self.ring_width = width;
            self.generation += 1;
        }
    }

    /// The ring back to the accent's color at the line step.
    pub(crate) fn reset_ring(&mut self) {
        self.set_ring_width(M.stroke.line);
        if self.ring_color.take().is_some() {
            self.generation += 1;
        }
    }

    pub(crate) const fn frosted(&self) -> bool {
        self.frosted
    }

    /// Frost the floating surfaces, or set them opaque as the stock themes are.
    pub(crate) const fn set_frosted(&mut self, frosted: bool) {
        if self.frosted != frosted {
            self.frosted = frosted;
            self.generation += 1;
        }
    }

    /// The filled call to action of `scheme`, over the palette its theme is built from.
    pub(crate) fn accent_button(&self, scheme: ColorScheme) -> ButtonTheme {
        ButtonTheme::accent(self.palette(scheme), &M)
    }

    /// Build both themes into `ui` when it has not seen this generation of edits; nothing on a
    /// frame with no edit, so a theme is never built per frame. Each window applies its own, as
    /// the main window and the inspector each hold their themes.
    pub(crate) fn apply(&self, ui: &mut Ui) {
        let id = WidgetId::from_hash("showcase::appearance::applied");
        ui.with_state::<u64, _>(id, |ui, applied| {
            if *applied == self.generation {
                return;
            }
            for scheme in [ColorScheme::Light, ColorScheme::Dark] {
                let palette = self.palette(scheme);
                let mut theme = Theme::from_parts(palette, &M);
                theme.focus_ring = self.ring(scheme);
                if self.frosted {
                    theme.frost_floats(palette, FROST_SIGMA, FROST_TINT);
                }
                ui.set_theme_for(scheme, theme);
            }
            *applied = self.generation;
        });
    }
}

/// `seed` as `scheme`'s accent: its Okhsv hue and saturation at the luminance of the stock
/// accent, so any seed sits where palantir's tuned accent sits against the ground and under
/// `on_accent`, the contrast the stock palettes already hold. A hue too dark to reach that
/// luminance at its saturation, as a deep blue in the dark scheme, gives up saturation instead,
/// toward white.
pub(crate) fn accent_for(seed: RgbaF32, scheme: ColorScheme) -> RgbaF32 {
    let luminance = Palette::for_scheme(scheme).accent.luminance();
    let Okhsv { h, s, .. } = Okhsv::from_color(seed, 0.0);
    let at = |s, v| Okhsv::new(h, s, v).to_color();
    if at(s, 1.0).luminance() >= luminance {
        // Luminance rises with the value at a fixed saturation: the lowest value that reaches it.
        let value = support::bisect(|v| at(s, v).luminance() >= luminance);
        at(s, value)
    } else {
        // At full value it falls as the saturation rises: the least cut of the saturation, toward
        // white, that reaches it.
        let cut = support::bisect(|cut| at(s * (1.0 - cut), 1.0).luminance() >= luminance);
        at(s * (1.0 - cut), 1.0)
    }
}

#[cfg(test)]
mod tests {
    use palantir::glam::UVec2;
    use palantir::{ColorScheme, Harness, Metrics, Okhsv, Palette, RgbaF32, Theme};

    use crate::appearance::{Appearance, SEEDS, accent_for};

    /// Any seed, solved for a scheme, lands on the stock accent's luminance, keeps its hue, and
    /// so holds the stock accent's contrast as an 8-bit target stores it: 4.5:1 against
    /// `on_accent` (WCAG 1.4.3), as a check mark and a switch knob sit on it, and 3:1 against the
    /// ground (WCAG 1.4.11), as the focus ring sits there. Pure blue cannot reach the dark
    /// luminance at its saturation, so it gives up saturation instead and keeps its hue.
    #[test]
    fn any_seed_holds_the_stock_accents_contrast() {
        let extra = [0x0000ff, 0xffff00, 0x808080, 0x000000, 0xffffff].map(RgbaF32::hex);
        for scheme in [ColorScheme::Light, ColorScheme::Dark] {
            let stock = Palette::for_scheme(scheme);
            for seed in SEEDS.into_iter().chain(extra) {
                let accent = accent_for(seed, scheme);
                // 24 halvings leave the solve within a few f32 steps of the luminance.
                let over = accent.luminance() - stock.accent.luminance();
                assert!((0.0..3e-7).contains(&over), "{scheme:?} {seed:?}: {over}");
                let stored = RgbaF32::from_srgba8(accent.to_srgba8());
                assert!(
                    stored.contrast_ratio(stock.on_accent) >= 4.5,
                    "{scheme:?} {seed:?}"
                );
                assert!(
                    stored.contrast_ratio(stock.window_bg) >= 3.0,
                    "{scheme:?} {seed:?}"
                );
                let hue = |color| Okhsv::from_color(color, 0.0).h;
                if Okhsv::from_color(seed, 0.0).s > 0.0 {
                    // The round trip through linear RGB holds the hue to a few f32 steps.
                    assert!(
                        (hue(accent) - hue(seed)).abs() < 1e-4,
                        "{scheme:?} {seed:?}"
                    );
                }
            }
        }
    }

    /// A seed, a ring or a frost edit moves the generation, so each window builds both themes
    /// once from the solved palettes: the ring follows the accent until a color of its own is set,
    /// a reset gives it back, and the floats are frosted until switched off.
    #[test]
    fn an_edit_builds_the_themes_once() {
        let mut h = Harness::builder(UVec2::new(64, 64)).mono().build();
        let mut look = Appearance::new();
        let scheme = h.ui().color_scheme();
        look.apply(h.ui());
        assert_eq!(h.ui().theme().focus_ring, look.ring(scheme));
        assert_eq!(
            h.ui().theme().focus_ring.color,
            Palette::for_scheme(scheme).accent
        );

        look.set_seed(Some(SEEDS[0]));
        look.apply(h.ui());
        let accent = accent_for(SEEDS[0], scheme);
        assert_eq!(look.palette(scheme).accent, accent);
        assert_eq!(look.palette(scheme).border_focused, accent);
        assert_eq!(h.ui().theme().focus_ring.color, accent);

        let red = RgbaF32::hex(0xff0000);
        look.set_ring_color(red);
        look.set_ring_width(3.0);
        look.apply(h.ui());
        assert_eq!(h.ui().theme().focus_ring.color, red);
        assert_eq!(h.ui().theme().focus_ring.width, 3.0);

        look.reset_ring();
        look.set_seed(None);
        look.apply(h.ui());
        assert_eq!(h.ui().theme().focus_ring, look.ring(scheme));
        assert_eq!(*look.palette(scheme), *Palette::for_scheme(scheme));

        // Frosted by default; off, the floats wear the stock opaque panel.
        assert!(!h.ui().theme().context_menu.panel.backdrop_filter.is_noop());
        look.set_frosted(false);
        look.apply(h.ui());
        let stock = Theme::from_parts(Palette::for_scheme(scheme), &Metrics::DEFAULT);
        assert_eq!(h.ui().theme().context_menu.panel, stock.context_menu.panel);
    }
}
