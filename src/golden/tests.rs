//! Pixel-diff coverage: what counts as differing, and what decides the
//! verdict.

use image::{Rgba, RgbaImage};

use crate::golden::Tolerance;

#[test]
fn identical_images_pass() {
    let img = RgbaImage::from_pixel(8, 8, Rgba([10, 20, 30, 255]));
    let report = Tolerance::default().diff(&img, &img);
    assert_eq!(report.max_channel_delta, 0);
    assert_eq!(report.differing_pixels, 0);
    assert!(report.passes());
}

#[test]
fn within_per_channel_tolerance_passes() {
    let a = RgbaImage::from_pixel(4, 4, Rgba([100, 100, 100, 255]));
    let e = RgbaImage::from_pixel(4, 4, Rgba([102, 100, 100, 255]));
    let report = Tolerance::default().diff(&a, &e);
    assert_eq!(report.max_channel_delta, 2);
    assert_eq!(report.differing_pixels, 0);
    assert!(report.passes());
}

#[test]
fn one_outlier_within_ratio_passes() {
    let mut a = RgbaImage::from_pixel(40, 40, Rgba([50, 50, 50, 255]));
    let e = RgbaImage::from_pixel(40, 40, Rgba([50, 50, 50, 255]));
    a.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
    // 1 differing pixel in 40x40 = a ratio of exactly 1/1600, so a
    // `max_ratio` of that admits it on the `<=` boundary.
    let tol = Tolerance {
        per_channel: 2,
        max_ratio: 1.0 / (40.0 * 40.0),
    };
    let report = tol.diff(&a, &e);
    assert!(report.max_channel_delta > 2);
    assert_eq!(report.differing_pixels, 1);
    assert!(report.passes());
}

#[test]
fn too_many_outliers_fail() {
    let a = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));
    let e = RgbaImage::from_pixel(8, 8, Rgba([255, 255, 255, 255]));
    let report = Tolerance::default().diff(&a, &e);
    assert_eq!(report.max_channel_delta, 255);
    assert_eq!(report.differing_pixels, 64);
    assert!(!report.passes());
}

#[test]
fn strict_tolerance_rejects_one_off() {
    let a = RgbaImage::from_pixel(2, 2, Rgba([100, 100, 100, 255]));
    let e = RgbaImage::from_pixel(2, 2, Rgba([101, 100, 100, 255]));
    let strict = Tolerance {
        per_channel: 0,
        max_ratio: 0.0,
    };
    let report = strict.diff(&a, &e);
    assert_eq!(report.max_channel_delta, 1);
    assert_eq!(report.differing_pixels, 4);
    assert!(!report.passes());
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

    let tol_loose = Tolerance {
        per_channel: 2,
        max_ratio: 0.02,
    };
    let loose = tol_loose.diff(&a, &e);
    assert_eq!(loose.max_channel_delta, 255);
    assert_eq!(loose.differing_pixels, 1);
    assert!(loose.passes());

    // Same pixels, same `per_channel`, tighter ratio — only the
    // ratio decides, so the verdict flips while the measurements
    // stay identical.
    let tol_tight = Tolerance {
        per_channel: 2,
        max_ratio: 0.005,
    };
    let tight = tol_tight.diff(&a, &e);
    assert_eq!(tight.max_channel_delta, 255);
    assert_eq!(tight.differing_pixels, 1);
    assert!(!tight.passes());
}

#[test]
fn per_channel_is_the_tolerance_the_report_was_measured_under() {
    // The skew `passes()` used to allow: a report measured with a
    // lenient `per_channel` counts zero differing pixels, and one
    // measured with a strict `per_channel` counts all of them —
    // from the same two images. Pinning that the verdict follows
    // the tolerance `diff` actually ran with.
    let a = RgbaImage::from_pixel(4, 4, Rgba([100, 100, 100, 255]));
    let e = RgbaImage::from_pixel(4, 4, Rgba([103, 100, 100, 255]));
    let ratio = 0.0;

    let lenient = Tolerance {
        per_channel: 4,
        max_ratio: ratio,
    }
    .diff(&a, &e);
    assert_eq!(lenient.differing_pixels, 0);
    assert!(lenient.passes());

    let strict = Tolerance {
        per_channel: 2,
        max_ratio: ratio,
    }
    .diff(&a, &e);
    assert_eq!(strict.differing_pixels, 16);
    assert!(!strict.passes());
}

#[test]
#[should_panic(expected = "image sizes differ")]
fn dimension_mismatch_panics() {
    let a = RgbaImage::new(4, 4);
    let e = RgbaImage::new(4, 5);
    let _ = Tolerance::default().diff(&a, &e);
}
