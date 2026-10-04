//! Golden-image regression testing: render, compare against a stored PNG,
//! and say precisely how they differ when they don't match.
//!
//! Kept here rather than in a test directory because more than one crate wants
//! it — Palantir's own visual suite, and anything drawing through Palantir that
//! wants the same workflow. Feature-gated so nothing pays for `image`
//! unless it asks.

mod row_stats;

/// The `image` Palantir was built against. Re-exported because this
/// module's surface takes and returns `image::RgbaImage`, and a suite naming
/// it from its own `image` dependency would have to keep the two
/// semver-identical by hand.
pub use image;

use std::path::{Path, PathBuf};

use crate::golden::row_stats::RowStats;
use image::RgbaImage;
use std::env;
use std::fs;
use std::io;
use std::process;
use std::thread;

/// How far an image may stray from what it is compared against, in the
/// shape of a WPT fuzzy match: at most `max_pixels` pixels may differ at
/// all, and none of them by more than `max_delta` on any R/G/B/A channel.
///
/// Both numbers bound something a reader can derive — how many pixels a
/// change may touch, and how wrong any one of them may be — so a loosened
/// golden says exactly what it lets through. [`Self::EXACT`], the default,
/// lets nothing through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tolerance {
    /// The largest channel deviation any pixel may carry.
    pub max_delta: u8,
    /// How many pixels may differ at all.
    pub max_pixels: u32,
}

impl Tolerance {
    /// No pixel may differ. Goldens written by the adapter that compares
    /// against them diff at zero on an unchanged tree, so anything looser
    /// hides a change.
    pub const EXACT: Self = Self {
        max_delta: 0,
        max_pixels: 0,
    };

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
                diff_image,
                tolerance: self,
            };
        }

        let row_bytes = w as usize * 4;
        let totals = actual
            .as_raw()
            .chunks_exact(row_bytes)
            .zip(expected.as_raw().chunks_exact(row_bytes))
            .zip(diff_image.chunks_exact_mut(row_bytes))
            .map(|((a_row, e_row), d_row)| RowStats::scan_row(a_row, e_row, d_row))
            .fold(RowStats::default(), RowStats::merge);
        DiffReport {
            max_channel_delta: totals.max_delta,
            differing_pixels: totals.differing,
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
    /// How many pixels differ at all, by any channel.
    pub differing_pixels: u32,
    /// The two images overlaid, with differing pixels marked.
    pub diff_image: RgbaImage,
    /// The tolerance the comparison ran under, which [`Self::passes`]
    /// reads.
    pub tolerance: Tolerance,
}

impl DiffReport {
    /// Whether no pixel strays further than the tolerance's `max_delta`,
    /// and no more pixels differ than its `max_pixels`.
    pub const fn passes(&self) -> bool {
        self.max_channel_delta <= self.tolerance.max_delta
            && self.differing_pixels <= self.tolerance.max_pixels
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
    adapter: Option<String>,
}

/// The sidecar beside the goldens that names the adapter which wrote them.
const ADAPTER_FILE: &str = "adapter.txt";

impl Goldens {
    /// Rooted at `root`, usually a suite's own directory under
    /// `CARGO_MANIFEST_DIR`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            tolerance: Tolerance::EXACT,
            adapter: None,
        }
    }

    /// Name the adapter this run draws on — its name and driver, as the
    /// suite's GPU reports them.
    ///
    /// The goldens record it in an `adapter.txt` sidecar the first time one
    /// is compared, and a later run on another adapter fails with that
    /// reason rather than with a pixel diff across the suite: a driver
    /// update or another machine is not a regression. An `UPDATE_GOLDEN` run
    /// adopts the new adapter.
    pub fn with_adapter(mut self, adapter: impl Into<String>) -> Self {
        self.adapter = Some(adapter.into());
        self
    }

    /// The golden files no name in `names` claims, sorted — goldens a test
    /// no longer compares against. A suite asserts this empty from one test
    /// that names every golden it draws.
    pub fn orphans<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> Vec<PathBuf> {
        let claimed: Vec<&str> = names.into_iter().collect();
        let Ok(entries) = fs::read_dir(self.root.join("golden")) else {
            return Vec::new();
        };
        let mut orphans: Vec<PathBuf> = entries
            .map(|entry| entry.expect("read golden directory entry").path())
            .filter(|path| {
                path.extension().is_some_and(|ext| ext == "png")
                    && path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .is_none_or(|stem| !claimed.contains(&stem))
            })
            .collect();
        orphans.sort();
        orphans
    }

    /// How far apart two images may drift and still pass.
    /// [`Tolerance::EXACT`] by default; a looser one names how many pixels
    /// and how far, with its derivation beside it.
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
        self.check_adapter(forced);
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

    /// Compare the run's adapter against the one the goldens record, and
    /// record it where none is recorded or an update adopts it.
    #[track_caller]
    fn check_adapter(&self, forced: bool) {
        let Some(adapter) = &self.adapter else {
            return;
        };
        let sidecar = self.root.join("golden").join(ADAPTER_FILE);
        match fs::read_to_string(&sidecar) {
            Ok(stored) if stored == *adapter => {}
            Ok(stored) if !forced => panic!(
                "the goldens in {} were written on adapter {stored:?}, and this run draws on \
                 {adapter:?}.\nRe-run with {UPDATE}=1 to adopt this adapter's goldens.",
                sidecar.parent().expect("sidecar has a directory").display()
            ),
            Ok(_) => Self::write_sidecar(&sidecar, adapter),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Self::write_sidecar(&sidecar, adapter);
            }
            Err(error) => panic!("read {}: {error}", sidecar.display()),
        }
    }

    /// Written beside and renamed into place, because a suite's tests
    /// compare in parallel: a plain write truncates the file first, and a
    /// test reading it then sees an empty adapter and fails on a mismatch
    /// that is not there.
    fn write_sidecar(sidecar: &Path, adapter: &str) {
        let dir = sidecar.parent().expect("sidecar has a directory");
        fs::create_dir_all(dir).expect("create golden directory");
        let staged = dir.join(format!(
            "{ADAPTER_FILE}.{}.{:?}.tmp",
            process::id(),
            thread::current().id()
        ));
        fs::write(&staged, adapter).expect("stage the adapter sidecar");
        fs::rename(&staged, sidecar).expect("move the adapter sidecar into place");
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
             differing pixels  {}\n  \
             allowed           {} per channel, {} pixels\n  \
             written to        {}",
            report.max_channel_delta,
            report.differing_pixels,
            self.tolerance.max_delta,
            self.tolerance.max_pixels,
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
