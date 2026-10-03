//! Golden-image regression testing: render, compare against a stored PNG,
//! and say precisely how they differ when they don't match.
//!
//! Kept here rather than in a test directory because more than one crate wants
//! it — Palantir's own visual suite, and anything drawing through Palantir that
//! wants the same workflow. Feature-gated so nothing pays for `image`
//! unless it asks.

mod row_stats;

use std::path::{Path, PathBuf};

use crate::golden::row_stats::RowStats;
use image::RgbaImage;
use std::env;
use std::fs;
use std::io;

/// Per-channel + ratio thresholds for [`Tolerance::diff`]. A pixel
/// "differs" when any R/G/B/A channel deviates by more than
/// `per_channel`; the image passes when the fraction of differing pixels
/// is at most `max_ratio`.
#[derive(Clone, Copy, Debug)]
pub struct Tolerance {
    /// Per-channel deviation a pixel may carry and still match.
    pub per_channel: u8,
    /// Fraction of differing pixels an image may carry and still pass.
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
    pub fn diff(self, actual: &RgbaImage, expected: &RgbaImage) -> DiffReport {
        // For the suites that pair two images themselves —
        // `assert_matches` screens the same mismatch first, with a
        // message that can name the golden, so this one never fires
        // from there.
        assert_eq!(
            actual.dimensions(),
            expected.dimensions(),
            "image sizes differ: actual {:?} vs expected {:?}",
            actual.dimensions(),
            expected.dimensions(),
        );
        let (w, h) = actual.dimensions();
        let mut diff_image = RgbaImage::new(w, h);

        // A pair covering no pixels differs nowhere, and the scan below
        // cannot be asked about one: `chunks_exact` rejects a
        // zero-length chunk, so a zero-width image panics there rather
        // than reporting anything. A zero-*height* one reaches the
        // end and divides by no pixels. One early answer covers both.
        if w == 0 || h == 0 {
            return DiffReport {
                max_channel_delta: 0,
                differing_pixels: 0,
                differing_ratio: 0.0,
                diff_image,
                tolerance: self,
            };
        }

        let row_bytes = w as usize * 4;
        let per_channel = self.per_channel;
        let totals = actual
            .as_raw()
            .chunks_exact(row_bytes)
            .zip(expected.as_raw().chunks_exact(row_bytes))
            .zip(diff_image.chunks_exact_mut(row_bytes))
            .map(|((a_row, e_row), d_row)| RowStats::scan_row(a_row, e_row, d_row, per_channel))
            .fold(RowStats::default(), RowStats::merge);

        // `u64` because the product overflows `u32` past 65 536², and the
        // divisor is nonzero by the guard above.
        let pixels = u64::from(w) * u64::from(h);
        DiffReport {
            max_channel_delta: totals.max_delta,
            differing_pixels: totals.differing,
            differing_ratio: totals.differing as f32 / pixels as f32,
            diff_image,
            tolerance: self,
        }
    }
}

/// What one [`Tolerance::diff`] measured.
#[derive(Debug)]
pub struct DiffReport {
    /// The largest single-channel deviation found anywhere.
    pub max_channel_delta: u8,
    /// How many pixels exceeded [`Tolerance::per_channel`].
    pub differing_pixels: u32,
    /// [`Self::differing_pixels`] over the image's pixel count.
    pub differing_ratio: f32,
    /// The two images overlaid, with differing pixels marked.
    pub diff_image: RgbaImage,
    /// The tolerance the comparison ran under. Carried rather than
    /// re-taken by [`Self::passes`]: `per_channel` is spent inside the
    /// scan deciding which pixels count as differing, so a `passes` that
    /// accepted its own `Tolerance` could only honour `max_ratio` and
    /// would silently pair one threshold with the other's ratio.
    pub tolerance: Tolerance,
}

impl DiffReport {
    /// Whether [`Self::differing_ratio`] is within the tolerance the
    /// comparison ran under.
    pub const fn passes(&self) -> bool {
        self.differing_ratio <= self.tolerance.max_ratio
    }
}

/// Set to anything non-empty to rewrite every golden the run touches, rather
/// than compare against it.
const UPDATE: &str = "UPDATE_GOLDEN";

/// A directory of golden images and the tolerance they are held to.
///
/// Goldens live at `<root>/golden/<name>.png`. A failure — against a
/// golden, or between two images through [`Self::assert_same`] — writes
/// what it actually got, what it expected, and a map of where they differ
/// to `<root>/output/<name>/`.
#[derive(Debug, Clone)]
#[must_use]
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
    pub const fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    fn golden_path(&self, name: &str) -> PathBuf {
        self.root.join("golden").join(format!("{name}.png"))
    }

    fn output_dir(&self, name: &str) -> PathBuf {
        self.root.join("output").join(name)
    }

    /// Compare `actual` against the golden called `name`, panicking with the
    /// measured difference if they disagree by more than the tolerance.
    ///
    /// A golden that doesn't exist yet is written and then *failed*. Passing
    /// instead would let a checkout with no goldens report success for every
    /// test in the suite, which is the one result this is here to prevent —
    /// and a first golden is exactly the image that most wants looking at
    /// before it becomes the thing everything else is judged against.
    ///
    /// With `UPDATE_GOLDEN` set, a missing or failing golden is rewritten
    /// from `actual` and passes; a passing one is left as it is, so an
    /// update run changes only what it has to. A pass clears whatever an
    /// earlier failure left under `output/<name>/`.
    #[track_caller]
    pub fn assert_matches(&self, name: &str, actual: &RgbaImage) {
        let forced = env::var_os(UPDATE).is_some_and(|value| !value.is_empty());
        self.check(name, actual, forced);
    }

    /// Compare `actual` against `expected`, two images the caller already
    /// holds, under the same tolerance: a replay of a scene, or one scene
    /// drawn two ways. A failure writes the pair and a map of where they
    /// differ to `output/<name>/` and panics, as a golden mismatch does. A
    /// pass clears whatever an earlier failure left there.
    ///
    /// # Panics
    ///
    /// Panics when the images differ by more than the tolerance, or differ
    /// in size.
    #[track_caller]
    pub fn assert_same(&self, name: &str, actual: &RgbaImage, expected: &RgbaImage) {
        let output = self.output_dir(name);
        let report = self.tolerance.diff(actual, expected);
        if report.passes() {
            Self::clear_output(&output);
            return;
        }
        let stats = self.record_failure(&output, actual, expected, &report);
        panic!("`{name}`: the two images differ:\n{stats}");
    }

    /// [`Self::assert_matches`] with the update flag passed in rather than
    /// read from the environment, which a test cannot set for itself alone.
    #[track_caller]
    fn check(&self, name: &str, actual: &RgbaImage, forced: bool) {
        let golden = self.golden_path(name);
        let output = self.output_dir(name);
        if !golden.exists() {
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
        let report = (actual.dimensions() == expected.dimensions())
            .then(|| self.tolerance.diff(actual, &expected));
        if report.as_ref().is_some_and(DiffReport::passes) {
            Self::clear_output(&output);
            return;
        }
        if forced {
            self.write(&golden, actual);
            Self::clear_output(&output);
            return;
        }
        let Some(report) = report else {
            panic!(
                "`{name}` is {:?}, golden is {:?} — a golden is only meaningful at one size",
                actual.dimensions(),
                expected.dimensions()
            );
        };

        let stats = self.record_failure(&output, actual, &expected, &report);
        panic!(
            "`{name}` does not match its golden:\n{stats}\n\
             Re-run with {UPDATE}=1 once the change is the one you wanted."
        );
    }

    /// Leave a failure's artifacts in `output` — what the test got, what it
    /// expected, and the map of where they differ — and describe the
    /// failure against this set's tolerance, for the panic.
    fn record_failure(
        &self,
        output: &Path,
        actual: &RgbaImage,
        expected: &RgbaImage,
        report: &DiffReport,
    ) -> String {
        fs::create_dir_all(output).expect("create golden output directory");
        actual.save(output.join("actual.png")).expect("save actual");
        expected
            .save(output.join("expected.png"))
            .expect("save expected");
        report
            .diff_image
            .save(output.join("diff.png"))
            .expect("save diff");
        format!(
            "  max channel delta {}\n  \
             differing pixels  {} ({:.4} of the image)\n  \
             allowed           {} per channel, {} of the image\n  \
             written to        {}",
            report.max_channel_delta,
            report.differing_pixels,
            report.differing_ratio,
            self.tolerance.per_channel,
            self.tolerance.max_ratio,
            output.display(),
        )
    }

    /// Remove a failure's artifacts, so `output/` names only what fails now.
    fn clear_output(output: &Path) {
        match fs::remove_dir_all(output) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => panic!("clear {}: {error}", output.display()),
        }
    }

    fn write(&self, golden: &Path, actual: &RgbaImage) {
        fs::create_dir_all(golden.parent().expect("golden path has a directory"))
            .expect("create golden directory");
        actual.save(golden).expect("save golden");
    }
}

#[cfg(test)]
mod tests;
