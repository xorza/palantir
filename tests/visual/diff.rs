//! Pixel-diff with per-channel + ratio tolerance.

use image::{Rgba, RgbaImage};

/// Per-channel + ratio thresholds for [`diff`]. A pixel "differs" when
/// any of its R/G/B/A channels deviates by more than `per_channel`;
/// the overall image fails when the fraction of differing pixels
/// exceeds `max_ratio`.
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
        self.max_channel_delta <= tol.per_channel || self.differing_ratio <= tol.max_ratio
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
    let mut max_delta: u8 = 0;
    let mut differing: u32 = 0;
    for ((a, e), d) in actual
        .pixels()
        .zip(expected.pixels())
        .zip(diff_image.pixels_mut())
    {
        let pixel_delta = (0..4).map(|c| a.0[c].abs_diff(e.0[c])).max().unwrap();
        if pixel_delta > max_delta {
            max_delta = pixel_delta;
        }
        if pixel_delta > tol.per_channel {
            differing += 1;
            *d = Rgba([255, 0, 0, 255]);
        } else {
            *d = Rgba([a.0[0] / 4, a.0[1] / 4, a.0[2] / 4, 255]);
        }
    }
    DiffReport {
        max_channel_delta: max_delta,
        differing_pixels: differing,
        differing_ratio: differing as f32 / (w * h) as f32,
        diff_image,
    }
}

#[cfg(test)]
mod tests {
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
    #[should_panic(expected = "image sizes differ")]
    fn dimension_mismatch_panics() {
        let a = RgbaImage::new(4, 4);
        let e = RgbaImage::new(4, 5);
        let _ = diff(&a, &e, Tolerance::default());
    }
}
