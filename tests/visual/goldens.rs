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

use image::{RgbaImage, imageops};
use palantir::Rect;
use palantir::golden::{Goldens, Tolerance};
use std::fs;
use std::thread;

/// The suite's directory: goldens under `golden/`, failures under
/// `output/`.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/visual");

/// Where a failure named `name` leaves its images, as `Goldens` lays the
/// directory out.
fn output_dir(name: &str) -> PathBuf {
    Path::new(ROOT).join("output").join(name)
}

/// No pixel may differ at all. The goldens are local and written by the
/// adapter that compares against them, so an unchanged tree diffs at zero;
/// anything looser would hide a change the suite exists to show.
const EXACT: Tolerance = Tolerance {
    per_channel: 0,
    max_ratio: 0.0,
};

pub(crate) fn assert_matches_golden(name: &str, actual: &RgbaImage) {
    assert_matches_golden_within(name, actual, EXACT);
}

/// [`assert_matches_golden`] under a looser tolerance — for a scene whose
/// pixels genuinely vary between runs on one adapter. The caller states
/// the derivation beside it.
pub(crate) fn assert_matches_golden_within(name: &str, actual: &RgbaImage, tolerance: Tolerance) {
    Goldens::new(ROOT)
        .tolerance(tolerance)
        .assert_matches(name, actual);
}

/// `actual` and `expected` agree in every pixel — `name` is a file name,
/// not a sentence.
#[track_caller]
pub(crate) fn assert_same(name: &str, actual: &RgbaImage, expected: &RgbaImage) {
    Goldens::new(ROOT)
        .tolerance(EXACT)
        .assert_same(name, actual, expected);
}

/// [`assert_same`] over the pixels of `region` only.
#[track_caller]
pub(crate) fn assert_same_in(name: &str, actual: &RgbaImage, expected: &RgbaImage, region: Rect) {
    assert_same(name, &crop(actual, region), &crop(expected, region));
}

/// The pixels of `image` inside `region`, whose edges must sit on whole
/// pixels.
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

/// Writes `image` to `output/<name>/actual.png` when the test panics while
/// this is alive: the artifact for a pixel assertion that has no expected
/// image to pair it with, in the place a golden failure leaves its own.
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
        // A failure to write must not turn the test's panic into an abort.
        if fs::create_dir_all(&output).is_ok() && self.image.save(output.join("actual.png")).is_ok()
        {
            eprintln!("`{}`: frame written to {}", self.name, output.display());
        }
    }
}
