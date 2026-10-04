//! Pixel-diff coverage: what counts as differing, and what decides the
//! verdict — and the golden directory's bookkeeping around it.

use std::path::PathBuf;

use image::{Rgba, RgbaImage};

use crate::golden::{Goldens, Tolerance};
use crate::internals::panic_probe;
use std::env;
use std::fs;
use std::process;
use std::sync::Barrier;
use std::thread;

/// A pair covering no pixels differs nowhere, so the verdict is a pass.
/// A zero *width* would panic inside `chunks_exact`, which rejects a
/// zero-length chunk, without the early answer.
#[test]
fn a_zero_pixel_pair_passes() {
    let empty = RgbaImage::new(0, 0);
    let report = Tolerance::default().diff(&empty, &empty);
    assert_eq!(report.differing_pixels, 0);
    assert!(report.passes());

    // A zero-width strip with real height, and the transpose: both
    // multiply to no pixels the same way.
    for (w, h) in [(0, 8), (8, 0)] {
        let strip = RgbaImage::new(w, h);
        let report = Tolerance::default().diff(&strip, &strip);
        assert_eq!(report.differing_pixels, 0, "{w}x{h}");
        assert!(report.passes(), "{w}x{h}");
    }
}

#[test]
fn identical_images_pass_exactly() {
    let img = RgbaImage::from_pixel(8, 8, Rgba([10, 20, 30, 255]));
    let report = Tolerance::EXACT.diff(&img, &img);
    assert_eq!(report.max_channel_delta, 0);
    assert_eq!(report.differing_pixels, 0);
    assert!(report.passes());
    assert_eq!(Tolerance::default(), Tolerance::EXACT);
}

/// A pixel differs when any channel differs at all, and the verdict
/// bounds both how many pixels differ and how far the worst one strays.
/// Here one pixel of 100 is three steps off on red: the count passes a
/// one-pixel budget, and the delta decides.
#[test]
fn the_verdict_bounds_both_the_count_and_the_delta() {
    let e = RgbaImage::from_pixel(10, 10, Rgba([100, 100, 100, 255]));
    let mut a = e.clone();
    a.put_pixel(3, 4, Rgba([103, 100, 100, 255]));

    let exact = Tolerance::EXACT.diff(&a, &e);
    assert_eq!(exact.max_channel_delta, 3);
    assert_eq!(exact.differing_pixels, 1);
    assert!(!exact.passes(), "exact admits no difference");

    for (max_delta, max_pixels, passes) in
        [(3, 1, true), (2, 1, false), (3, 0, false), (255, 1, true)]
    {
        let tolerance = Tolerance {
            max_delta,
            max_pixels,
        };
        assert_eq!(tolerance.diff(&a, &e).passes(), passes, "{tolerance:?}");
    }
}

/// The diff map marks every differing pixel, however small the step, and
/// dims the rest of `actual` to a quarter.
#[test]
fn the_diff_map_marks_every_differing_pixel() {
    let e = RgbaImage::from_pixel(2, 1, Rgba([100, 100, 100, 255]));
    let mut a = e.clone();
    a.put_pixel(1, 0, Rgba([101, 100, 100, 255]));
    let report = Tolerance::EXACT.diff(&a, &e);
    assert_eq!(*report.diff_image.get_pixel(0, 0), Rgba([25, 25, 25, 255]));
    assert_eq!(*report.diff_image.get_pixel(1, 0), Rgba([255, 0, 0, 255]));
}

#[test]
#[should_panic(expected = "image sizes differ")]
fn dimension_mismatch_panics() {
    let a = RgbaImage::new(4, 4);
    let e = RgbaImage::new(4, 5);
    let _ = Tolerance::default().diff(&a, &e);
}

/// A directory of its own under the system temp dir, gone on drop.
#[derive(Debug)]
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = env::temp_dir().join(format!("palantir-golden-{name}-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// An update run rewrites what is missing or failing and leaves a passing
/// golden alone; a pass clears an earlier failure's output; without the
/// flag, a failure panics and writes its artifacts, and a missing golden
/// is written and failed. Two held images compared through `assert_same`
/// take the same handling, and write no golden.
///
/// `near` is one step off `base` in all 16 pixels, inside a tolerance of
/// two steps over 16 pixels; `far` is forty steps off, outside it.
#[test]
fn failures_leave_artifacts_updates_rewrite_and_passes_clear() {
    let dir = Scratch::new("update");
    let goldens = Goldens::new(&dir.0).with_tolerance(Tolerance {
        max_delta: 2,
        max_pixels: 16,
    });
    let solid = |v: u8| RgbaImage::from_pixel(4, 4, Rgba([v, 10, 10, 255]));
    let (base, near, far) = (solid(10), solid(11), solid(50));
    let golden = goldens.golden_path("g");
    let output = dir.0.join("output").join("g");
    let stored = || image::open(&golden).unwrap().to_rgba8();

    goldens.check("g", &base, true);
    assert_eq!(stored(), base, "a missing golden is written by an update");

    goldens.check("g", &near, true);
    assert_eq!(stored(), base, "a passing golden is left as it is");

    fs::create_dir_all(&output).unwrap();
    goldens.check("g", &near, false);
    assert!(
        !output.exists(),
        "a pass clears an earlier failure's output"
    );

    goldens.check("g", &far, true);
    assert_eq!(stored(), far, "a failing golden is rewritten by an update");

    panic_probe::assert_panics_with("`g` does not match its golden", || {
        goldens.check("g", &base, false);
    });
    assert!(
        output.join("actual.png").exists(),
        "and leaves its artifacts"
    );
    assert_eq!(stored(), far, "and keeps the golden");

    panic_probe::assert_panics_with("no golden for `h`", || goldens.check("h", &base, false));
    assert!(goldens.golden_path("h").exists(), "after it is written");

    let pair = dir.0.join("output").join("pair");
    fs::create_dir_all(&pair).unwrap();
    goldens.assert_same("pair", &near, &base);
    assert!(!pair.exists(), "a pair that passes clears its old output");
    panic_probe::assert_panics_with("differing pixels  16", || {
        goldens.assert_same("pair", &far, &base);
    });
    for (file, written) in [("actual.png", &far), ("expected.png", &base)] {
        assert_eq!(
            image::open(pair.join(file)).unwrap().to_rgba8(),
            *written,
            "{file}"
        );
    }
    assert!(pair.join("diff.png").exists(), "and the diff map");
    assert!(
        !goldens.golden_path("pair").exists(),
        "comparing two images writes no golden"
    );
}

/// The adapter sidecar: the first comparison records the run's adapter, a
/// later run on the same one compares as usual, a run on another adapter
/// fails with that reason before any pixel diff, and an update run adopts
/// the new adapter.
#[test]
fn goldens_record_and_hold_their_adapter() {
    let dir = Scratch::new("adapter");
    let image = RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]));
    let sidecar = dir.0.join("golden").join("adapter.txt");
    let first = Goldens::new(&dir.0).with_adapter("GPU A");
    first.check("g", &image, true);
    assert_eq!(fs::read_to_string(&sidecar).unwrap(), "GPU A");
    first.check("g", &image, false);

    let other = Goldens::new(&dir.0).with_adapter("GPU B");
    panic_probe::assert_panics_with("written on adapter \"GPU A\"", || {
        other.check("g", &image, false);
    });
    other.check("g", &image, true);
    assert_eq!(
        fs::read_to_string(&sidecar).unwrap(),
        "GPU B",
        "an update adopts it"
    );

    Goldens::new(&dir.0).check("g", &image, false);

    // Tests compare in parallel, so a run that finds no sidecar has many
    // writers and many readers at once. Each reader sees the whole adapter
    // or no file, never a write half done.
    let names: Vec<String> = (0..8).map(|i| format!("t{i}")).collect();
    let racing = Goldens::new(&dir.0).with_adapter("GPU C");
    for name in &names {
        racing.check(name, &image, true);
    }
    let sidecar_dir = sidecar.parent().unwrap();
    for _ in 0..200 {
        fs::remove_file(&sidecar).unwrap();
        let start = Barrier::new(names.len());
        thread::scope(|scope| {
            for name in &names {
                scope.spawn(|| {
                    start.wait();
                    racing.check(name, &image, false);
                });
            }
        });
        assert_eq!(fs::read_to_string(&sidecar).unwrap(), "GPU C");
    }
    let staged = fs::read_dir(sidecar_dir)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|e| e == "tmp")
        })
        .count();
    assert_eq!(staged, 0, "every staged sidecar is renamed into place");
}

/// Every golden no name claims is an orphan, in name order; the adapter
/// sidecar is not a golden, and a set with no directory has none.
#[test]
fn orphans_are_the_goldens_no_name_claims() {
    let dir = Scratch::new("orphans");
    let goldens = Goldens::new(&dir.0).with_adapter("GPU A");
    assert!(goldens.orphans([]).is_empty(), "no directory, no orphans");
    let image = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]));
    for name in ["kept", "old", "older"] {
        goldens.check(name, &image, true);
    }
    assert_eq!(
        goldens.orphans(["kept"]),
        [goldens.golden_path("old"), goldens.golden_path("older")],
    );
    assert!(goldens.orphans(["kept", "old", "older"]).is_empty());
}
