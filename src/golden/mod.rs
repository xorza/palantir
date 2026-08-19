//! Golden-image regression testing: render, compare against a committed PNG,
//! and say precisely how they differ when they don't match.
//!
//! Kept here rather than in a test directory because more than one crate wants
//! it — Palantir's own visual suite, and anything drawing through Palantir that
//! wants the same workflow. Feature-gated so nothing pays for `image` and
//! `rayon` unless it asks.

pub(crate) mod diff;

use std::path::{Path, PathBuf};

use image::RgbaImage;

pub use crate::golden::diff::{DiffReport, Tolerance, diff};

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

        let report = diff(actual, &expected, self.tolerance);
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
