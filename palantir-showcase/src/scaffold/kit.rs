//! [`Kit`]: what a page paints with this frame, and the specimen swatches each scheme solves.

use palantir::{ColorCoords, ColorModel, ColorScheme, Palette, RgbaF32};

use crate::scaffold::bisect;
use crate::scaffold::per_scheme::PerScheme;
use crate::scaffold::tokens::Tokens;

/// What a page paints with this frame: the scaffold's tokens and the shown scheme's swatches.
/// Pages take it as a parameter. Refused: a kit in a state row or a static, which hides what a
/// page paints with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Kit {
    pub(crate) t: Tokens,
    pub(crate) swatch: Swatches,
}

/// Both schemes' swatches, solved once, and the kit of whichever scheme shows.
#[derive(Debug)]
pub(crate) struct Kits {
    swatches: PerScheme<Swatches>,
}

impl Default for Kits {
    fn default() -> Self {
        Self {
            swatches: PerScheme::of(Swatches::solve),
        }
    }
}

impl Kits {
    /// The kit of `scheme`, over `palette`, the one its theme is built from.
    pub(crate) const fn kit(&self, scheme: ColorScheme, palette: &Palette) -> Kit {
        Kit {
            t: Tokens::of(palette),
            swatch: *self.swatches.get(scheme),
        }
    }
}

/// The specimen colors of one scheme: five hues at one saturation and one relative luminance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Swatches {
    pub(crate) teal: RgbaF32,
    pub(crate) orange: RgbaF32,
    pub(crate) lime: RgbaF32,
    pub(crate) violet: RgbaF32,
    pub(crate) red: RgbaF32,
}

/// What a scheme's swatches are solved to.
#[derive(Clone, Copy, Debug)]
struct Target {
    /// The Okhsv saturation every hue takes.
    saturation: f32,
    /// The relative luminance every swatch lands on.
    luminance: f32,
}

impl Target {
    /// Dark: the highest luminance every hue reaches at the saturation, as violet tops out at
    /// 0.368 there. Light: between the floor [`SWATCH_INK`](crate::scaffold::SWATCH_INK) sets and
    /// the ceiling the light ground sets. The tests hold each swatch to 3:1 against its ground
    /// and 4.5:1 under [`SWATCH_INK`](crate::scaffold::SWATCH_INK) (WCAG 1.4.11 and 1.4.3).
    const fn of(scheme: ColorScheme) -> Self {
        match scheme {
            ColorScheme::Dark => Self {
                saturation: 0.6,
                luminance: 0.35,
            },
            ColorScheme::Light => Self {
                saturation: 0.75,
                luminance: 0.25,
            },
        }
    }

    fn color(self, hue: f32) -> RgbaF32 {
        let at =
            |value| ColorCoords::new(ColorModel::Okhsv, hue, self.saturation, value).to_color();
        debug_assert!(
            at(1.0).luminance() >= self.luminance,
            "hue {hue} cannot reach the target luminance"
        );
        at(bisect(|value| at(value).luminance() >= self.luminance))
    }
}

impl Swatches {
    /// The Okhsv hues of the tour's first, neon set, in turns: teal, orange, lime, violet, red.
    const HUES: [f32; 5] = [0.62, 0.18, 0.34, 0.87, 0.09];

    /// Each hue at the scheme's saturation, its value solved by bisection to the scheme's
    /// luminance: one luminance gives every swatch one contrast against the ground and under
    /// [`SWATCH_INK`](crate::scaffold::SWATCH_INK), as WCAG measures contrast in luminance, and
    /// Okhsv keeps the hue and the gamut as the value moves. Refused: one Oklab lightness, which leaves the contrast per hue;
    /// hand-picked hex, which a palette change leaves behind.
    pub(crate) fn solve(scheme: ColorScheme) -> Self {
        let target = Target::of(scheme);
        let [teal, orange, lime, violet, red] = Self::HUES.map(|hue| target.color(hue));
        Self {
            teal,
            orange,
            lime,
            violet,
            red,
        }
    }

    pub(crate) const fn all(self) -> [RgbaF32; 5] {
        [self.teal, self.orange, self.lime, self.violet, self.red]
    }
}
