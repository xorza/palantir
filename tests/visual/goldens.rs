//! This suite's golden directory, bound once so a fixture names only its image.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and colour channels"
)]
#![expect(
    clippy::print_stderr,
    reason = "a rewritten golden is reported to the terminal so the run says which files changed"
)]

use std::path::{Path, PathBuf};

use glam::UVec2;

use crate::golden_name::GoldenName;
use crate::harness::Harness;
use palantir::golden::Goldens;
use palantir::golden::image::{RgbaImage, imageops};
use palantir::internals::headless_test_gpu;
use palantir::{Rect, Ui};
use std::fs;
use std::thread;

/// The suite's directory: goldens under `golden/`, failures under `output/`.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/visual");

/// Where a failure named `name` leaves its images in `Goldens`' layout.
fn output_dir(name: &str) -> PathBuf {
    Path::new(ROOT).join("output").join(name)
}

/// The suite's goldens, held to `Tolerance::EXACT` and to the adapter that wrote them. They are local, so an unchanged tree diffs at zero and another adapter fails with that reason instead of suite-wide pixel diffs.
pub(crate) fn goldens() -> Goldens {
    Goldens::new(ROOT).with_adapter(headless_test_gpu().adapter.clone())
}

#[track_caller]
pub(crate) fn assert_matches_golden(golden: GoldenName, actual: &RgbaImage) {
    goldens().assert_matches(golden.name(), actual);
}

/// One frame of `scene` at `size` on a fresh [`Harness`] matches `golden`.
#[track_caller]
pub(crate) fn assert_scene_matches_golden(
    golden: GoldenName,
    size: UVec2,
    scene: impl FnMut(&mut Ui),
) {
    assert_settled_scene_matches_golden(golden, size, 0, scene);
}

/// [`assert_scene_matches_golden`] on the frame after `settle` discarded ones, for state that builds over frames.
#[track_caller]
pub(crate) fn assert_settled_scene_matches_golden(
    golden: GoldenName,
    size: UVec2,
    settle: u32,
    scene: impl FnMut(&mut Ui),
) {
    let img = Harness::new().size(size).settled_frame(settle, scene).image;
    assert_matches_golden(golden, &img);
}

/// `actual` and `expected` agree in every pixel; `name` is a file name, not a sentence.
#[track_caller]
pub(crate) fn assert_same(name: &str, actual: &RgbaImage, expected: &RgbaImage) {
    Goldens::new(ROOT).assert_same(name, actual, expected);
}

/// [`assert_same`] over the pixels of `region` only.
#[track_caller]
pub(crate) fn assert_same_in(name: &str, actual: &RgbaImage, expected: &RgbaImage, region: Rect) {
    assert_same(name, &crop(actual, region), &crop(expected, region));
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

/// Writes `image` to `output/<name>/actual.png` if the test panics while this is alive: the artifact for a pixel assertion with no expected image.
#[derive(Debug)]
pub(crate) struct KeptOnFailure<'a> {
    name: &'a str,
    image: &'a RgbaImage,
}

impl<'a> KeptOnFailure<'a> {
    pub(crate) const fn new(name: &'a str, image: &'a RgbaImage) -> Self {
        Self { name, image }
    }
}

impl Drop for KeptOnFailure<'_> {
    fn drop(&mut self) {
        if !thread::panicking() {
            return;
        }
        let output = output_dir(self.name);
        // A failed write must not turn the panic into an abort.
        if fs::create_dir_all(&output).is_ok() && self.image.save(output.join("actual.png")).is_ok()
        {
            eprintln!("`{}`: frame written to {}", self.name, output.display());
        }
    }
}
