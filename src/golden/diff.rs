//! Pixel-diff with per-channel + ratio tolerance. Per-row parallel
//! via rayon; rows are independent so the reduction is a trivial
//! `(max, sum)`.

use image::RgbaImage;
use rayon::prelude::*;

/// Per-channel + ratio thresholds for [`diff`]. A pixel "differs" when
/// any R/G/B/A channel deviates by more than `per_channel`; the image
/// passes when the fraction of differing pixels is ≤ `max_ratio`.
#[derive(Clone, Copy, Debug)]
pub struct Tolerance {
    pub per_channel: u8,
    pub max_ratio: f32,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            per_channel: 2,
            max_ratio: 0.001,
        }
    }
}

#[derive(Debug)]
pub struct DiffReport {
    pub max_channel_delta: u8,
    pub differing_pixels: u32,
    pub differing_ratio: f32,
    pub diff_image: RgbaImage,
}

impl DiffReport {
    pub fn passes(&self, tol: Tolerance) -> bool {
        self.differing_ratio <= tol.max_ratio
    }
}

/// Compare two equal-sized RGBA images. The diff image marks each
/// differing pixel red (alpha 255) and dims the rest of the `actual`
/// image to 25% so failures pop visually.
pub fn diff(actual: &RgbaImage, expected: &RgbaImage, tol: Tolerance) -> DiffReport {
    assert_eq!(
        actual.dimensions(),
        expected.dimensions(),
        "image sizes differ: actual {:?} vs expected {:?}",
        actual.dimensions(),
        expected.dimensions(),
    );
    let (w, h) = actual.dimensions();
    let mut diff_image = RgbaImage::new(w, h);

    let row_bytes = w as usize * 4;
    let per_channel = tol.per_channel;
    let totals = actual
        .as_raw()
        .par_chunks_exact(row_bytes)
        .zip(expected.as_raw().par_chunks_exact(row_bytes))
        .zip(diff_image.par_chunks_exact_mut(row_bytes))
        .map(|((a_row, e_row), d_row)| diff_row(a_row, e_row, d_row, per_channel))
        .reduce(RowStats::default, RowStats::merge);

    DiffReport {
        max_channel_delta: totals.max_delta,
        differing_pixels: totals.differing,
        differing_ratio: totals.differing as f32 / (w * h) as f32,
        diff_image,
    }
}

#[derive(Default, Clone, Copy)]
struct RowStats {
    max_delta: u8,
    differing: u32,
}

impl RowStats {
    fn merge(a: Self, b: Self) -> Self {
        Self {
            max_delta: a.max_delta.max(b.max_delta),
            differing: a.differing + b.differing,
        }
    }
}

/// Scan one row: writes each diff pixel into `d_row` (red on miss,
/// dimmed actual on match) and returns the row's `(max_delta, count)`.
fn diff_row(a_row: &[u8], e_row: &[u8], d_row: &mut [u8], per_channel: u8) -> RowStats {
    let mut stats = RowStats::default();
    for ((a, e), d) in a_row
        .chunks_exact(4)
        .zip(e_row.chunks_exact(4))
        .zip(d_row.chunks_exact_mut(4))
    {
        let delta = (0..4).map(|c| a[c].abs_diff(e[c])).max().unwrap();
        if delta > stats.max_delta {
            stats.max_delta = delta;
        }
        if delta > per_channel {
            stats.differing += 1;
            d.copy_from_slice(&[255, 0, 0, 255]);
        } else {
            d[0] = a[0] / 4;
            d[1] = a[1] / 4;
            d[2] = a[2] / 4;
            d[3] = 255;
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    #[test]
    fn identical_images_pass() {
        let img = RgbaImage::from_pixel(8, 8, Rgba([10, 20, 30, 255]));
        let report = diff(&img, &img, Tolerance::default());
        assert_eq!(report.max_channel_delta, 0);
        assert_eq!(report.differing_pixels, 0);
        assert!(report.passes(Tolerance::default()));
    }

    #[test]
    fn within_per_channel_tolerance_passes() {
        let a = RgbaImage::from_pixel(4, 4, Rgba([100, 100, 100, 255]));
        let e = RgbaImage::from_pixel(4, 4, Rgba([102, 100, 100, 255]));
        let report = diff(&a, &e, Tolerance::default());
        assert_eq!(report.max_channel_delta, 2);
        assert_eq!(report.differing_pixels, 0);
        assert!(report.passes(Tolerance::default()));
    }

    #[test]
    fn one_outlier_within_ratio_passes() {
        let mut a = RgbaImage::from_pixel(40, 40, Rgba([50, 50, 50, 255]));
        let e = RgbaImage::from_pixel(40, 40, Rgba([50, 50, 50, 255]));
        a.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        let report = diff(&a, &e, Tolerance::default());
        assert!(report.max_channel_delta > 2);
        assert_eq!(report.differing_pixels, 1);
        let tol = Tolerance {
            per_channel: 2,
            max_ratio: 1.0 / (40.0 * 40.0),
        };
        assert!(report.passes(tol));
    }

    #[test]
    fn too_many_outliers_fail() {
        let a = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));
        let e = RgbaImage::from_pixel(8, 8, Rgba([255, 255, 255, 255]));
        let report = diff(&a, &e, Tolerance::default());
        assert_eq!(report.max_channel_delta, 255);
        assert_eq!(report.differing_pixels, 64);
        assert!(!report.passes(Tolerance::default()));
    }

    #[test]
    fn strict_tolerance_rejects_one_off() {
        let a = RgbaImage::from_pixel(2, 2, Rgba([100, 100, 100, 255]));
        let e = RgbaImage::from_pixel(2, 2, Rgba([101, 100, 100, 255]));
        let strict = Tolerance {
            per_channel: 0,
            max_ratio: 0.0,
        };
        let report = diff(&a, &e, strict);
        assert_eq!(report.max_channel_delta, 1);
        assert_eq!(report.differing_pixels, 4);
        assert!(!report.passes(strict));
    }

    #[test]
    fn ratio_gates_pass_regardless_of_outlier_magnitude() {
        // One saturated outlier in 100 pixels = 0.01 ratio.
        // Pin that `passes` is ratio-only — a giant per-pixel delta
        // doesn't fail the report so long as the count stays below
        // `max_ratio`.
        let mut a = RgbaImage::from_pixel(10, 10, Rgba([0, 0, 0, 255]));
        let e = RgbaImage::from_pixel(10, 10, Rgba([0, 0, 0, 255]));
        a.put_pixel(0, 0, Rgba([255, 255, 255, 255]));
        let report = diff(&a, &e, Tolerance::default());
        assert_eq!(report.max_channel_delta, 255);
        assert_eq!(report.differing_pixels, 1);
        let tol_loose = Tolerance {
            per_channel: 2,
            max_ratio: 0.02,
        };
        assert!(report.passes(tol_loose));
        let tol_tight = Tolerance {
            per_channel: 2,
            max_ratio: 0.005,
        };
        assert!(!report.passes(tol_tight));
    }

    #[test]
    #[should_panic(expected = "image sizes differ")]
    fn dimension_mismatch_panics() {
        let a = RgbaImage::new(4, 4);
        let e = RgbaImage::new(4, 5);
        let _ = diff(&a, &e, Tolerance::default());
    }
}
