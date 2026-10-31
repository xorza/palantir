//! [`Appearance`]: the palettes and steps the example's themes are built from, which the
//! appearance study edits.

use std::fmt::Write as _;

use palantir::{ColorScheme, Metrics, Palette, RgbaF32, Theme, Ui};

/// A starting point for the palettes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Preset {
    /// Palantir's built-in light and dark palettes.
    Stock,
    /// Red on black in the dark slot, the scheme pinned dark: red light keeps the eye's dark
    /// adaptation, which bright light takes up to 40 minutes to restore, as Stellarium's night
    /// mode does (<https://www.stellarium-labs.com/blog/nightmode>).
    NightVision,
}

/// Red on near-black. Its text clears 4.5:1 on every surface; its rungs sit closer than the
/// stock palettes' 1.10:1, as any brighter ground would cost the dark adaptation it keeps.
pub(crate) const NIGHT_VISION: Palette = Palette {
    text: RgbaF32::hex(0xff8a7a),
    text_muted: RgbaF32::hex(0xe0584a),
    text_disabled: RgbaF32::hex(0x7a2a22),
    window_bg: RgbaF32::hex(0x0a0000),
    surround: RgbaF32::hex(0x1a0302),
    element: RgbaF32::hex(0x2a0605),
    element_mid: RgbaF32::hex(0x380907),
    element_strong: RgbaF32::hex(0x4a0c09),
    border_focused: RgbaF32::hex(0xff4433),
    accent: RgbaF32::hex(0xff4433),
    on_accent: RgbaF32::hex(0x0a0000),
    shadow: RgbaF32::BLACK.with_alpha(0.5),
};

/// The palettes and steps the themes are built from, and whether the themes still need building.
#[derive(Debug)]
pub(crate) struct Appearance {
    light: Palette,
    dark: Palette,
    pub(crate) metrics: Metrics,
    preset: Preset,
    /// Set by every edit; [`Self::apply`] builds the themes once and clears it.
    stale: bool,
}

impl Appearance {
    /// The stock palettes and steps, not yet applied.
    pub(crate) const fn new() -> Self {
        Self {
            light: Palette::LIGHT,
            dark: Palette::DARK,
            metrics: Metrics::DEFAULT,
            preset: Preset::Stock,
            stale: true,
        }
    }

    pub(crate) const fn palette(&self, scheme: ColorScheme) -> &Palette {
        match scheme {
            ColorScheme::Light => &self.light,
            ColorScheme::Dark => &self.dark,
        }
    }

    /// The palette of `scheme`, for an edit; the themes rebuild on the next [`Self::apply`].
    pub(crate) const fn palette_mut(&mut self, scheme: ColorScheme) -> &mut Palette {
        self.stale = true;
        match scheme {
            ColorScheme::Light => &mut self.light,
            ColorScheme::Dark => &mut self.dark,
        }
    }

    /// The steps, for an edit; the themes rebuild on the next [`Self::apply`].
    pub(crate) const fn metrics_mut(&mut self) -> &mut Metrics {
        self.stale = true;
        &mut self.metrics
    }

    pub(crate) const fn preset(&self) -> Preset {
        self.preset
    }

    /// Start over from `preset`: its palettes and the stock steps.
    pub(crate) const fn set_preset(&mut self, preset: Preset) {
        self.preset = preset;
        self.light = Palette::LIGHT;
        self.dark = match preset {
            Preset::Stock => Palette::DARK,
            Preset::NightVision => NIGHT_VISION,
        };
        self.metrics = Metrics::DEFAULT;
        self.stale = true;
    }

    /// Build both themes into `ui` when an edit made them stale; nothing on a frame with no edit,
    /// so a theme is never built per frame.
    pub(crate) fn apply(&mut self, ui: &mut Ui) {
        if !self.stale {
            return;
        }
        for scheme in [ColorScheme::Light, ColorScheme::Dark] {
            ui.set_theme_for(
                scheme,
                Theme::from_parts(self.palette(scheme), &self.metrics),
            );
        }
        self.stale = false;
    }

    /// The palette of `scheme` and the steps as Rust source, for an app to paste.
    pub(crate) fn rust_source(&self, scheme: ColorScheme) -> String {
        let p = self.palette(scheme);
        let mut out = String::from("Palette {\n");
        for (name, color) in [
            ("text", p.text),
            ("text_muted", p.text_muted),
            ("text_disabled", p.text_disabled),
            ("window_bg", p.window_bg),
            ("surround", p.surround),
            ("element", p.element),
            ("element_mid", p.element_mid),
            ("element_strong", p.element_strong),
            ("border_focused", p.border_focused),
            ("accent", p.accent),
            ("on_accent", p.on_accent),
            ("shadow", p.shadow),
        ] {
            let c = color.to_srgba8();
            if c.a == u8::MAX {
                writeln!(
                    out,
                    "    {name}: RgbaF32::hex(0x{:02x}{:02x}{:02x}),",
                    c.r, c.g, c.b
                )
            } else {
                writeln!(
                    out,
                    "    {name}: RgbaF32::hexa(0x{:02x}{:02x}{:02x}{:02x}),",
                    c.r, c.g, c.b, c.a
                )
            }
            .expect("a String takes every write");
        }
        let m = &self.metrics;
        write!(
            out,
            "}}\nMetrics {{\n    text: {:?},\n    space: {:?},\n    radius: {:?},\n    \
             stroke: {:?},\n    ..Metrics::DEFAULT\n}}\n",
            m.text, m.space, m.radius, m.stroke
        )
        .expect("a String takes every write");
        out
    }
}

#[cfg(test)]
mod tests {
    use palantir::{ColorScheme, Metrics, Palette};

    use crate::app::model::appearance::{Appearance, NIGHT_VISION, Preset};

    /// Night vision's text reads at 4.5:1 or more on each surface it can sit on.
    #[test]
    fn night_vision_text_clears_its_floor() {
        let p = NIGHT_VISION;
        for ground in [
            p.window_bg,
            p.surround,
            p.element,
            p.element_mid,
            p.element_strong,
        ] {
            assert!(p.text.contrast_ratio(ground) >= 4.5, "{ground:?}");
        }
        for ground in [p.window_bg, p.surround, p.element, p.element_mid] {
            assert!(p.text_muted.contrast_ratio(ground) >= 4.5, "{ground:?}");
        }
        assert!(p.on_accent.contrast_ratio(p.accent) >= 4.5);
    }

    /// A preset swaps the dark slot only and resets the steps; the stock palettes come back.
    #[test]
    fn a_preset_swaps_the_dark_palette() {
        let mut look = Appearance::new();
        look.metrics_mut().text.body = 20.0;
        look.set_preset(Preset::NightVision);
        assert_eq!(*look.palette(ColorScheme::Dark), NIGHT_VISION);
        assert_eq!(*look.palette(ColorScheme::Light), Palette::LIGHT);
        assert_eq!(look.metrics, Metrics::DEFAULT);
        look.set_preset(Preset::Stock);
        assert_eq!(*look.palette(ColorScheme::Dark), Palette::DARK);
    }

    /// The source names every role in its hex, alpha as `hexa`, and the steps as struct literals.
    #[test]
    fn the_rust_source_holds_each_value() {
        let look = Appearance::new();
        let source = look.rust_source(ColorScheme::Dark);
        assert!(source.starts_with("Palette {\n    text: RgbaF32::hex(0xebebeb),\n"));
        assert!(source.contains("    accent: RgbaF32::hex(0x44b5ff),\n"));
        assert!(source.contains("    shadow: RgbaF32::hexa(0x00000080),\n"));
        assert!(
            source.contains("    text: TextSteps { caption: 12.0, body: 14.0, title: 16.0 },\n")
        );
        assert!(source.ends_with("    ..Metrics::DEFAULT\n}\n"));
    }
}
