//! This suite's golden directory, bound once so a fixture names only its image.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and color channels"
)]

use palantir::Rect;
use palantir::golden::Goldens;
use palantir::golden::image::{RgbaImage, imageops};
use palantir::internals::headless_test_gpu;

/// The suite's directory: goldens under `golden/`, failures under `output/`.
pub(crate) const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/visual");

/// The suite's goldens, held to `Tolerance::EXACT` and to the adapter that wrote them. They are local, so an unchanged tree diffs at zero and another adapter fails with that reason instead of suite-wide pixel diffs.
pub(crate) fn suite() -> Goldens {
    Goldens::new(ROOT).with_adapter(headless_test_gpu().adapter.clone())
}

/// `actual` and `expected` agree in every pixel; `name` is a file name, not a sentence.
#[track_caller]
pub(crate) fn assert_same(name: &str, actual: &RgbaImage, expected: &RgbaImage) {
    Goldens::new(ROOT).assert_same(name, actual, expected);
}

/// The pixels of `image` inside `region`, whose edges must be whole pixels.
pub(crate) fn crop(image: &RgbaImage, region: Rect) -> RgbaImage {
    imageops::crop_imm(
        image,
        region.min.x as u32,
        region.min.y as u32,
        region.size.w as u32,
        region.size.h as u32,
    )
    .to_image()
}
