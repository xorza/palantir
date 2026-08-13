//! This suite's golden directory, bound once so a fixture names only its image.

use image::RgbaImage;
use palantir::golden::{Goldens, Tolerance};

pub(crate) fn assert_matches_golden(name: &str, actual: &RgbaImage, tolerance: Tolerance) {
    Goldens::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/visual"))
        .tolerance(tolerance)
        .assert_matches(name, actual);
}
