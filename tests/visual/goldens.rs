//! This suite's golden directory, bound once so a fixture names only its image.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and color channels"
)]

use image::{RgbaImage, imageops};
use palantir::glam::UVec2;
use palantir::golden::Goldens;
use palantir::internals::headless_test_gpu;
use palantir::{Image, Rect};
use std::sync::OnceLock;

/// The suite's directory: goldens under `golden/`, failures under `output/`.
pub(crate) const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/visual");

/// The suite's goldens, held to `Tolerance::EXACT` and to the adapter that wrote them. They are local, so an unchanged tree diffs at zero and another adapter fails with that reason instead of suite-wide pixel diffs. Built once, as naming the adapter leases the GPU.
pub(crate) fn suite() -> &'static Goldens {
    static SUITE: OnceLock<Goldens> = OnceLock::new();
    SUITE.get_or_init(|| Goldens::new(ROOT).with_adapter(headless_test_gpu().adapter.clone()))
}

/// `actual` and `expected` agree in every pixel; `name` is a file name, not a sentence.
#[track_caller]
pub(crate) fn assert_same(name: &str, actual: &RgbaImage, expected: &RgbaImage) {
    suite().assert_same(name, &to_image(actual), &to_image(expected));
}

/// A readback as palantir's [`Image`], the form the golden comparison reads.
pub(crate) fn to_image(rgba: &RgbaImage) -> Image {
    Image::from_srgba8(
        UVec2::new(rgba.width(), rgba.height()),
        rgba.as_raw().clone(),
    )
    .expect("a readback is a non-empty RGBA8 buffer")
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
