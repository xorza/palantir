//! Golden-image regression testing: render, compare against a committed PNG,
//! and say precisely how they differ when they don't match.
//!
//! Kept here rather than in a test directory because more than one crate wants
//! it — Palantir's own visual suite, and anything drawing through Palantir that
//! wants the same workflow. Feature-gated so nothing pays for `image` and
//! `rayon` unless it asks.

mod row_stats;

use std::path::{Path, PathBuf};

use crate::golden::row_stats::RowStats;
use image::RgbaImage;
use rayon::prelude::*;

/// Per-channel + ratio thresholds for [`Tolerance::diff`]. A pixel
/// "differs" when any R/G/B/A channel deviates by more than
/// `per_channel`; the image passes when the fraction of differing pixels
/// is at most `max_ratio`.
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

impl Tolerance {
    /// Compare two equal-sized RGBA images under these thresholds. The
    /// diff image marks each differing pixel red (alpha 255) and dims the
    /// rest of the `actual` image to 25% so failures pop visually.
    ///
    /// Per-row parallel via rayon; rows are independent, so the reduction
    /// is a trivial `(max, sum)`.
    pub fn diff(self, actual: &RgbaImage, expected: &RgbaImage) -> DiffReport {
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
        let per_channel = self.per_channel;
        let totals = actual
            .as_raw()
            .par_chunks_exact(row_bytes)
            .zip(expected.as_raw().par_chunks_exact(row_bytes))
            .zip(diff_image.par_chunks_exact_mut(row_bytes))
            .map(|((a_row, e_row), d_row)| RowStats::scan_row(a_row, e_row, d_row, per_channel))
            .reduce(RowStats::default, RowStats::merge);

        DiffReport {
            max_channel_delta: totals.max_delta,
            differing_pixels: totals.differing,
            differing_ratio: totals.differing as f32 / (w * h) as f32,
            diff_image,
            tolerance: self,
        }
    }
}

/// What one [`Tolerance::diff`] measured.
#[derive(Debug)]
pub struct DiffReport {
    pub max_channel_delta: u8,
    pub differing_pixels: u32,
    pub differing_ratio: f32,
    pub diff_image: RgbaImage,
    /// The tolerance the comparison ran under. Carried rather than
    /// re-taken by [`Self::passes`]: `per_channel` is spent inside the
    /// scan deciding which pixels count as differing, so a `passes` that
    /// accepted its own `Tolerance` could only honour `max_ratio` and
    /// would silently pair one threshold with the other's ratio.
    pub tolerance: Tolerance,
}

impl DiffReport {
    pub fn passes(&self) -> bool {
        self.differing_ratio <= self.tolerance.max_ratio
    }
}

/// Set to anything non-empty to rewrite every golden the run touches, rather
/// than compare against it.
const UPDATE: &str = "UPDATE_GOLDEN";

/// A directory of golden images and the tolerance they are held to.
///
/// Goldens live at `<root>/golden/<name>.png` and are meant to be committed.
/// A failure writes what it actually got, what it expected, and a map of where
/// they differ to `<root>/output/<name>/`, which is meant to be ignored.
#[derive(Debug, Clone)]
pub struct Goldens {
    root: PathBuf,
    tolerance: Tolerance,
}

impl Goldens {
    /// Rooted at `root`, usually a suite's own directory under
    /// `CARGO_MANIFEST_DIR`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            tolerance: Tolerance::default(),
        }
    }

    /// How far apart two images may drift and still pass.
    ///
    /// The default suits flat, mostly axis-aligned drawing. A scene made of
    /// antialiased curves wants a looser ratio: the edge pixels are where two
    /// runs disagree, and a curve is nearly all edge.
    pub fn tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    fn golden_path(&self, name: &str) -> PathBuf {
        self.root.join("golden").join(format!("{name}.png"))
    }

    /// Compare `actual` against the golden called `name`, panicking with the
    /// measured difference if they disagree by more than the tolerance.
    ///
    /// A golden that doesn't exist yet is written and then *failed*. Passing
    /// instead would let a checkout with no goldens report success for every
    /// test in the suite, which is the one result this is here to prevent —
    /// and a first golden is exactly the image that most wants looking at
    /// before it becomes the thing everything else is judged against.
    pub fn assert_matches(&self, name: &str, actual: &RgbaImage) {
        let golden = self.golden_path(name);
        let forced = std::env::var_os(UPDATE).is_some_and(|value| !value.is_empty());
        if forced || !golden.exists() {
            self.write(&golden, actual);
            if forced {
                return;
            }
            panic!(
                "no golden for `{name}` — wrote {}.\nLook at it, and re-run if it is what you meant to draw.",
                golden.display()
            );
        }

        let expected = image::open(&golden)
            .unwrap_or_else(|error| panic!("read golden {}: {error}", golden.display()))
            .to_rgba8();
        if actual.dimensions() != expected.dimensions() {
            panic!(
                "`{name}` is {:?}, golden is {:?} — a golden is only meaningful at one size",
                actual.dimensions(),
                expected.dimensions()
            );
        }

        let report = self.tolerance.diff(actual, &expected);
        if report.passes() {
            return;
        }

        let output = self.root.join("output").join(name);
        std::fs::create_dir_all(&output).expect("create golden output directory");
        actual.save(output.join("actual.png")).expect("save actual");
        expected
            .save(output.join("expected.png"))
            .expect("save expected");
        report
            .diff_image
            .save(output.join("diff.png"))
            .expect("save diff");

        panic!(
            "`{name}` does not match its golden:\n  \
             max channel delta {}\n  \
             differing pixels  {} ({:.4} of the image)\n  \
             allowed           {} per channel, {} of the image\n  \
             written to        {}\n\
             Re-run with {UPDATE}=1 once the change is the one you wanted.",
            report.max_channel_delta,
            report.differing_pixels,
            report.differing_ratio,
            self.tolerance.per_channel,
            self.tolerance.max_ratio,
            output.display(),
        );
    }

    fn write(&self, golden: &Path, actual: &RgbaImage) {
        std::fs::create_dir_all(golden.parent().expect("golden path has a directory"))
            .expect("create golden directory");
        actual.save(golden).expect("save golden");
    }
}

#[cfg(test)]
mod tests;
