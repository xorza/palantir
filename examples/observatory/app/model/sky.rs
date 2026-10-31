//! How the sky reads on the chart: where an object lands, its color from its B−V index, its size
//! from its magnitude, and its coordinates as text.

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::fmt::{self, Display, Formatter};

use palantir::RgbaF32;
use palantir::glam::Vec2;

/// The chart's size in logical pixels at zoom 1: 10 px a degree of RA and of Dec.
pub(crate) const CHART: Vec2 = Vec2::new(3600.0, 1800.0);

/// Where `(ra, dec)` lands on the chart, a plate carrée with RA growing to the left, as the sky
/// looks from below.
pub(crate) fn chart_point(ra: f32, dec: f32) -> Vec2 {
    Vec2::new((TAU - ra) / TAU * CHART.x, (FRAC_PI_2 - dec) / PI * CHART.y)
}

/// The `(ra, dec)` at chart point `at`, the inverse of [`chart_point`], RA wrapped to `0..TAU`.
pub(crate) fn sky_at(at: Vec2) -> (f32, f32) {
    let ra = (TAU - at.x / CHART.x * TAU).rem_euclid(TAU);
    let dec = (FRAC_PI_2 - at.y / CHART.y * PI).clamp(-FRAC_PI_2, FRAC_PI_2);
    (ra, dec)
}

/// The temperature of a star of color index `bv`, by Ballesteros' black-body fit
/// (F. J. Ballesteros, "New insights into black bodies", EPL 97 (2012) 34008,
/// <https://arxiv.org/abs/1201.1809>), in kelvin.
pub(crate) fn temperature(bv: f32) -> f32 {
    4600.0 * (1.0 / (0.92 * bv + 1.7) + 1.0 / (0.92 * bv + 0.62))
}

/// The linear-sRGB color of a black body at `kelvin`, the largest channel 1: its chromaticity by
/// the Kim et al. cubic fit of the Planckian locus, valid 1667–25000 K
/// (<https://en.wikipedia.org/wiki/Planckian_locus#Approximation>), then CIE XYZ at Y = 1 to
/// linear sRGB through the D65 matrix (IEC 61966-2-1).
pub(crate) fn black_body(kelvin: f32) -> RgbaF32 {
    // f64: the fit's published coefficients carry more digits than an f32 holds.
    let t = f64::from(kelvin).clamp(1667.0, 25000.0);
    let (t2, t3) = (t * t, t * t * t);
    let x = if t <= 4000.0 {
        -0.266_123_9e9 / t3 - 0.234_358_9e6 / t2 + 0.877_695_6e3 / t + 0.179_910
    } else {
        -3.025_846_9e9 / t3 + 2.107_037_9e6 / t2 + 0.222_634_7e3 / t + 0.240_390
    };
    let (x2, x3) = (x * x, x * x * x);
    let y = if t <= 2222.0 {
        -1.106_381_4 * x3 - 1.348_110_2 * x2 + 2.185_558_32 * x - 0.202_196_83
    } else if t <= 4000.0 {
        -0.954_947_6 * x3 - 1.374_185_93 * x2 + 2.091_370_15 * x - 0.167_488_67
    } else {
        3.081_758 * x3 - 5.873_386_7 * x2 + 3.751_129_97 * x - 0.370_014_83
    };
    let (cx, cz) = (x / y, (1.0 - x - y) / y);
    let r = 3.240_454_2 * cx - 1.537_138_5 - 0.498_531_4 * cz;
    let g = -0.969_266 * cx + 1.876_010_8 + 0.041_556 * cz;
    let b = 0.055_643_4 * cx - 0.204_025_9 + 1.057_225_2 * cz;
    let (r, g, b) = (r.max(0.0), g.max(0.0), b.max(0.0));
    let peak = r.max(g).max(b);
    RgbaF32::new((r / peak) as f32, (g / peak) as f32, (b / peak) as f32, 1.0)
}

/// A star's color from its B−V index: [`black_body`] at its [`temperature`].
pub(crate) fn star_color(bv: f32) -> RgbaF32 {
    black_body(temperature(bv))
}

/// The magnitude a disc of [`FAINT_RADIUS`] stands for, the faintest the generator draws.
const FAINT_MAG: f32 = 6.5;
/// The radius of the faintest star, logical pixels at zoom 1.
const FAINT_RADIUS: f32 = 0.7;
/// The largest disc, so a bright star stays a point and not a blot.
const MAX_RADIUS: f32 = 5.5;

/// A star's disc radius at zoom 1: flux goes as `10^(−0.4 m)` and the disc's area with it, so
/// the radius goes as `10^(−0.2 m)`, from [`FAINT_RADIUS`] at magnitude 6.5 up to [`MAX_RADIUS`].
pub(crate) fn star_radius(mag: f32) -> f32 {
    (FAINT_RADIUS * 10_f32.powf(-0.2 * (mag - FAINT_MAG))).min(MAX_RADIUS)
}

/// Right ascension as hours, minutes and whole seconds: `05h 35m 17s`; a time that rounds to 24h
/// reads `00h 00m 00s`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hms(pub(crate) f32);

impl Display for Hms {
    #[expect(
        clippy::cast_sign_loss,
        reason = "`rem_euclid` holds the seconds non-negative"
    )]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let seconds = (self.0.rem_euclid(TAU) / TAU * 86_400.0).round() as u32 % 86_400;
        let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
        write!(f, "{h:02}h {m:02}m {s:02}s")
    }
}

/// Declination as signed degrees and whole arcminutes: `−05° 23′`. The sign is the value's, so a
/// declination just south of the equator keeps its minus (`−00° 20′`); one that rounds to zero
/// reads `+00° 00′`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Dms(pub(crate) f32);

impl Display for Dms {
    #[expect(clippy::cast_sign_loss, reason = "the minutes count an absolute value")]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let minutes = (self.0.abs() / PI * 180.0 * 60.0).round() as u32;
        let sign = if self.0 < 0.0 && minutes > 0 {
            '\u{2212}'
        } else {
            '+'
        };
        write!(f, "{sign}{:02}° {:02}′", minutes / 60, minutes % 60)
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, PI};

    use palantir::glam::Vec2;

    use crate::app::model::sky::{
        CHART, Dms, Hms, black_body, chart_point, sky_at, star_radius, temperature,
    };

    /// The chart's corners and center: RA 0 at the right edge, 12h mid, Dec +90° at the top; and
    /// the inverse lands back on each point.
    #[test]
    fn the_chart_projection_and_its_inverse() {
        assert_eq!(chart_point(0.0, FRAC_PI_2), Vec2::new(CHART.x, 0.0));
        assert_eq!(chart_point(PI, 0.0), CHART * 0.5);
        assert_eq!(
            chart_point(PI, -FRAC_PI_2),
            Vec2::new(CHART.x * 0.5, CHART.y)
        );
        let (ra, dec) = sky_at(Vec2::new(900.0, 450.0));
        assert!((ra - 1.5 * PI).abs() < 1e-5 && (dec - FRAC_PI_2 / 2.0).abs() < 1e-5);
        assert_eq!(sky_at(Vec2::new(CHART.x, 0.0)), (0.0, FRAC_PI_2));
    }

    /// The Sun's index, 0.65, reads its 5778 K by Ballesteros' fit:
    /// `4600 · (1/2.298 + 1/1.218)`; index 0 reads 10 125 K.
    #[test]
    fn ballesteros_reads_the_suns_temperature() {
        assert!(
            (temperature(0.65) - 5778.4).abs() < 0.5,
            "{}",
            temperature(0.65)
        );
        assert!(
            (temperature(0.0) - 10_125.0).abs() < 1.0,
            "{}",
            temperature(0.0)
        );
    }

    /// The black body at D65's correlated temperature, 6504 K, sits on the locus at
    /// (0.31343, 0.32360), where D65 itself sits 0.003 above it, so it reads a touch magenta:
    /// (1, 0.94389, 0.99366) by the same arithmetic in f64. A cool star is red-led, a hot one
    /// blue-led.
    #[test]
    fn the_black_body_runs_from_red_through_white_to_blue() {
        let d65 = black_body(6504.0);
        for (got, want) in [(d65.r, 1.0), (d65.g, 0.943_887), (d65.b, 0.993_664)] {
            assert!((got - want).abs() < 1e-5, "{d65:?}");
        }
        let cool = black_body(3000.0);
        assert!(
            cool.r == 1.0 && cool.b < cool.g && cool.g < cool.r,
            "{cool:?}"
        );
        let hot = black_body(20_000.0);
        assert!(hot.b == 1.0 && hot.r < hot.b, "{hot:?}");
    }

    /// Faintest 0.7 px; five magnitudes brighter is ten times wider, so magnitude 1.5 caps at 5.5.
    #[test]
    fn star_radius_follows_the_flux_law() {
        assert!((star_radius(6.5) - 0.7).abs() < 1e-6);
        assert!((star_radius(4.0) - 0.7 * 10_f32.sqrt()).abs() < 1e-5);
        assert_eq!(star_radius(1.5), 5.5);
    }

    /// Coordinates as text, at their edges: a time that rounds up to 24h wraps, a declination
    /// just south of the equator keeps its sign, one that rounds to zero does not.
    #[test]
    fn coordinates_read_as_catalogs_write_them() {
        let hours = |h: f32| h / 12.0 * PI;
        let degrees = |d: f32| d / 180.0 * PI;
        assert_eq!(
            Hms(hours(5.0 + 35.0 / 60.0 + 17.0 / 3600.0)).to_string(),
            "05h 35m 17s"
        );
        assert_eq!(
            Hms(hours(23.0 + 59.0 / 60.0 + 59.6 / 3600.0)).to_string(),
            "00h 00m 00s"
        );
        assert_eq!(
            Dms(degrees(-(5.0 + 23.0 / 60.0))).to_string(),
            "\u{2212}05° 23′"
        );
        assert_eq!(Dms(degrees(-20.0 / 60.0)).to_string(), "\u{2212}00° 20′");
        assert_eq!(Dms(degrees(-0.1 / 60.0)).to_string(), "+00° 00′");
        assert_eq!(Dms(degrees(89.999)).to_string(), "+90° 00′");
    }
}
