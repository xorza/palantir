//! The tour's look a person sets: the accent, and the focus page's ring, over the stock palettes,
//! built into each window's themes once an edit.

use palantir::{
    AnimationSpec, ButtonTheme, ColorCoords, ColorModel, ColorScheme, FocusRingTheme, Metrics,
    Palette, RgbaF32, Theme, Ui, WidgetId,
};

use crate::support;

const M: Metrics = Metrics::DEFAULT;

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
    light: Palette,
    dark: Palette,
    /// Each seed of [`SEEDS`] solved for the light and the dark scheme, once, for the rail to show.
    light_seeds: [RgbaF32; SEEDS.len()],
    dark_seeds: [RgbaF32; SEEDS.len()],
    /// Moved by every edit; each window builds its themes once a generation, as each window's
    /// `Ui` holds its own.
    generation: u64,
    /// How the last edit came, which picks how its themes show.
    edit: Edit,
}

/// How an edit reaches the look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edit {
    /// A pick: a click on a swatch, a toggle, a reset. Its themes blend in under
    /// [`AnimationSpec::THEME`].
    Pick,
    /// A drag that sets a value every frame, a slider's or a color field's. Its themes snap, as a
    /// blend would trail the pointer.
    Drag,
}

impl Appearance {
    /// The stock look, not yet applied.
    pub(crate) fn new() -> Self {
        Self {
            seed: None,
            ring_color: None,
            ring_width: M.stroke.line,
            light: Palette::LIGHT,
            dark: Palette::DARK,
            light_seeds: SEEDS.map(|seed| accent_for(seed, ColorScheme::Light)),
            dark_seeds: SEEDS.map(|seed| accent_for(seed, ColorScheme::Dark)),
            generation: 1,
            edit: Edit::Pick,
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
        self.edited(Edit::Pick);
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
            self.edited(Edit::Drag);
        }
    }

    pub(crate) fn set_ring_width(&mut self, width: f32) {
        if self.ring_width != width {
            self.ring_width = width;
            self.edited(Edit::Drag);
        }
    }

    /// The ring back to the accent's color at the line step.
    pub(crate) fn reset_ring(&mut self) {
        let width = self.ring_width != M.stroke.line;
        self.ring_width = M.stroke.line;
        if self.ring_color.take().is_some() || width {
            self.edited(Edit::Pick);
        }
    }

    /// A new generation of edits, the last one made by `edit`.
    const fn edited(&mut self, edit: Edit) {
        self.generation += 1;
        self.edit = edit;
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
            ui.set_theme_transition(match self.edit {
                Edit::Pick => AnimationSpec::THEME,
                Edit::Drag => AnimationSpec::SNAP,
            });
            for scheme in [ColorScheme::Light, ColorScheme::Dark] {
                let palette = self.palette(scheme);
                let mut theme = Theme::from_parts(palette, &M);
                theme.focus_ring = self.ring(scheme);
                ui.set_theme_for(scheme, theme);
            }
            *applied = self.generation;
        });
    }
}

/// The floors an accent holds on `scheme`'s stock palette, as palantir's floors test pairs it,
/// read as an 8-bit target stores it: WCAG 1.4.3's 4.5:1 under `on_accent`, which a check mark
/// and a switch knob paint on it, and WCAG 1.4.11's 3:1 as the focus ring on the ground, the band
/// and a field's fill.
fn holds_floors(accent: RgbaF32, p: &Palette) -> bool {
    let stored = RgbaF32::from_srgba8(accent.to_srgba8());
    stored.contrast_ratio(p.on_accent) >= 4.5
        && [p.window_bg, p.surround, p.element_mid]
            .into_iter()
            .all(|ground| stored.contrast_ratio(ground) >= 3.0)
}

/// `seed` as `scheme`'s accent: the seed itself when it holds the floors, else the nearest color
/// at its Okhsv hue and saturation that does, so a vivid seed stays vivid. Under the dark
/// palette's dark ink the floors bound the luminance from below, so the value rises to the
/// first that holds; a hue too dark to hold them at full value gives up saturation instead,
/// toward white. Under the light palette's white ink they bound it from above, and the value
/// falls. Material 3 derives its tones from one seed in the same way. Refused: one luminance per
/// scheme, the stock accent's, which paled every seed under the dark palette's lime; presets
/// alone, as GNOME and macOS offer, which drop "Use as accent"; a failing seed as given, as
/// Windows' custom accent, which can leave the tour unreadable.
pub(crate) fn accent_for(seed: RgbaF32, scheme: ColorScheme) -> RgbaF32 {
    // An accent paints opaque, whatever alpha the picker gave the seed.
    let seed = seed.with_alpha(1.0);
    let p = Palette::for_scheme(scheme);
    let holds = |color| holds_floors(color, p);
    if holds(seed) {
        return seed;
    }
    let axes = ColorCoords::from_color(seed, ColorModel::Okhsv, 0.0);
    let (s, v) = (axes.saturation(), axes.value());
    let at = |s, v| axes.with_saturation(s).with_value(v).to_color();
    if p.on_accent.luminance() < p.accent.luminance() {
        if holds(at(s, 1.0)) {
            let rise = |x: f32| v + (1.0 - v) * x;
            at(s, rise(support::bisect(|x| holds(at(s, rise(x))))))
        } else {
            let cut = |x: f32| s * (1.0 - x);
            at(cut(support::bisect(|x| holds(at(cut(x), 1.0)))), 1.0)
        }
    } else {
        let fall = |x: f32| v * (1.0 - x);
        at(s, fall(support::bisect(|x| holds(at(s, fall(x))))))
    }
}

#[cfg(test)]
mod tests {
    use palantir::glam::UVec2;
    use palantir::{
        AnimationSpec, ColorCoords, ColorModel, ColorScheme, Harness, Metrics, Palette, RgbaF32,
        Theme,
    };

    use crate::appearance::{Appearance, SEEDS, accent_for, holds_floors};

    /// Any seed, solved for a scheme, holds the floors and keeps its hue; one that holds them
    /// already stays as given, and one that does not moves no further than the floors ask: a
    /// thousandth of the value back toward the seed misses them. Pure blue cannot hold the dark
    /// floors at its saturation, so it gives up saturation and keeps its hue.
    #[test]
    fn any_seed_holds_the_floors_nearest_to_itself() {
        let extra = [0x0000ff, 0xffff00, 0x808080, 0x000000, 0xffffff].map(RgbaF32::hex);
        for scheme in [ColorScheme::Light, ColorScheme::Dark] {
            let p = Palette::for_scheme(scheme);
            for seed in SEEDS.into_iter().chain(extra) {
                let accent = accent_for(seed, scheme);
                assert!(holds_floors(accent, p), "{scheme:?} {seed:?}");
                if holds_floors(seed, p) {
                    assert_eq!(accent, seed, "{scheme:?}: a seed that holds stays");
                    continue;
                }
                let seed_hsv = ColorCoords::from_color(seed, ColorModel::Okhsv, 0.0);
                let got = ColorCoords::from_color(accent, ColorModel::Okhsv, 0.0);
                if seed_hsv.saturation() > 0.0 {
                    // The round trip through linear RGB holds the hue to a few f32 steps.
                    assert!(
                        (got.hue() - seed_hsv.hue()).abs() < 1e-4,
                        "{scheme:?} {seed:?}"
                    );
                }
                let back = (seed_hsv.value() - got.value()).signum() * 1e-3;
                if (got.saturation() - seed_hsv.saturation()).abs() < 1e-4 && back != 0.0 {
                    let nearer = got.with_value(got.value() + back).to_color();
                    assert!(
                        !holds_floors(nearer, p),
                        "{scheme:?} {seed:?}: not the nearest"
                    );
                }
            }
        }
        // Tailwind's blue 500 reads 2.61:1 on the dark field's fill, under 3:1, so it lightens;
        // its teal 500 reads 7.40:1 under the ink and 3.85:1 on the fill, so it stays.
        assert_ne!(accent_for(SEEDS[0], ColorScheme::Dark), SEEDS[0]);
        assert_eq!(accent_for(SEEDS[1], ColorScheme::Dark), SEEDS[1]);
        assert_eq!(
            accent_for(SEEDS[1].with_alpha(0.25), ColorScheme::Dark),
            SEEDS[1],
            "a translucent seed paints opaque"
        );
        let okhsv = |color| ColorCoords::from_color(color, ColorModel::Okhsv, 0.0);
        let blue = okhsv(accent_for(RgbaF32::hex(0x0000ff), ColorScheme::Dark));
        assert!(blue.saturation() < okhsv(RgbaF32::hex(0x0000ff)).saturation());
    }

    /// A seed or a ring edit moves the generation, so each window builds both themes once from
    /// the solved palettes: the ring follows the accent until a color of its own is set, and a
    /// reset gives it back. A pick's themes blend in and a drag's snap. The floats wear the
    /// stock frost.
    #[test]
    fn an_edit_builds_the_themes_once() {
        let mut h = Harness::builder(UVec2::new(64, 64)).mono().build();
        let mut look = Appearance::new();
        let scheme = h.ui().color_scheme();
        look.apply(h.ui_mut());
        assert_eq!(h.ui().theme().focus_ring, look.ring(scheme));
        assert_eq!(
            h.ui().theme().focus_ring.color,
            Palette::for_scheme(scheme).accent
        );

        look.set_seed(Some(SEEDS[0]));
        look.apply(h.ui_mut());
        let accent = accent_for(SEEDS[0], scheme);
        assert_eq!(look.palette(scheme).accent, accent);
        assert_eq!(look.palette(scheme).border_focused, accent);
        assert_eq!(h.ui().theme().focus_ring.color, accent);
        assert_eq!(h.ui().theme_transition(), AnimationSpec::THEME, "a pick");

        let red = RgbaF32::hex(0xff0000);
        look.set_ring_color(red);
        look.set_ring_width(3.0);
        look.apply(h.ui_mut());
        assert_eq!(h.ui().theme().focus_ring.color, red);
        assert_eq!(h.ui().theme().focus_ring.width, 3.0);
        assert_eq!(h.ui().theme_transition(), AnimationSpec::SNAP, "a drag");

        look.reset_ring();
        look.set_seed(None);
        look.apply(h.ui_mut());
        assert_eq!(h.ui().theme_transition(), AnimationSpec::THEME, "a reset");
        assert_eq!(h.ui().theme().focus_ring, look.ring(scheme));
        assert_eq!(*look.palette(scheme), *Palette::for_scheme(scheme));

        let stock = Theme::from_parts(Palette::for_scheme(scheme), &Metrics::DEFAULT);
        assert!(!stock.menu.panel.backdrop_filter.is_noop());
        assert_eq!(h.ui().theme().menu.panel, stock.menu.panel);
    }
}
