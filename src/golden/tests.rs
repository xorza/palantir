//! Pixel-diff coverage: what counts as differing, and what decides the
//! verdict — and the golden directory's bookkeeping around it.

use std::path::PathBuf;

use crate::golden::{Goldens, OnMismatch, Tolerance, load};
use crate::internals::panic_probe;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::Image;
use glam::UVec2;
use std::env;
use std::fs;
use std::process;
use std::sync::Barrier;
use std::thread;

/// A `w`×`h` image of one texel.
fn solid(w: u32, h: u32, [r, g, b, a]: [u8; 4]) -> Image {
    let mut image = Image::blank(UVec2::new(w, h));
    image.texels_mut().fill(SrgbaU8::new(r, g, b, a));
    image
}

/// `image` with the texel at column `x`, row `y` set to `rgba`.
fn with_texel(mut image: Image, x: u32, y: u32, [r, g, b, a]: [u8; 4]) -> Image {
    let idx = (y * image.size().x + x) as usize;
    image.texels_mut()[idx] = SrgbaU8::new(r, g, b, a);
    image
}

#[test]
fn identical_images_pass_exactly() {
    let img = solid(8, 8, [10, 20, 30, 255]);
    let report = Tolerance::EXACT.diff(&img, &img);
    assert_eq!(report.max_channel_delta, 0);
    assert_eq!(report.differing_pixels, 0);
    assert!(report.passes());
    assert_eq!(Tolerance::default(), Tolerance::EXACT);
}

/// A pixel differs on any channel difference; the verdict bounds the differing count and the worst delta.
#[test]
fn the_verdict_bounds_both_the_count_and_the_delta() {
    let e = solid(10, 10, [100, 100, 100, 255]);
    let a = with_texel(e.clone(), 3, 4, [103, 100, 100, 255]);

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

/// The diff map marks every differing pixel and dims the rest of `actual` to a quarter.
#[test]
fn the_diff_map_marks_every_differing_pixel() {
    let e = solid(2, 1, [100, 100, 100, 255]);
    let a = with_texel(e.clone(), 1, 0, [101, 100, 100, 255]);
    let report = Tolerance::EXACT.diff(&a, &e);
    assert_eq!(
        report.diff_image.texels(),
        [SrgbaU8::new(25, 25, 25, 255), SrgbaU8::new(255, 0, 0, 255)]
    );
}

#[test]
#[should_panic(expected = "image sizes differ")]
fn dimension_mismatch_panics() {
    let a = Image::blank(UVec2::new(4, 4));
    let e = Image::blank(UVec2::new(4, 5));
    let _ = Tolerance::default().diff(&a, &e);
}

/// A directory of its own under the system temp dir, gone on drop.
#[derive(Debug)]
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = env::temp_dir().join(format!("palantir-golden-{name}-{}", process::id()));
        fs::remove_dir_all(&dir).ok();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// An update run rewrites missing or failing goldens and leaves passing ones; a pass clears earlier failure output; without the flag a failure panics with artifacts and a missing golden is written then failed. `assert_same` behaves alike and writes no golden.
#[test]
fn failures_leave_artifacts_updates_rewrite_and_passes_clear() {
    let dir = Scratch::new("update");
    let goldens = Goldens::new(&dir.0).with_tolerance(Tolerance {
        max_delta: 2,
        max_pixels: 16,
    });
    let red = |v: u8| solid(4, 4, [v, 10, 10, 255]);
    let (base, near, far) = (red(10), red(11), red(50));
    let golden = goldens.golden_path("g");
    let output = dir.0.join("output").join("g");
    let stored = || load(&golden);

    goldens.check("g", &base, OnMismatch::Rewrite);
    assert_eq!(stored(), base, "a missing golden is written by an update");

    goldens.check("g", &near, OnMismatch::Rewrite);
    assert_eq!(stored(), base, "a passing golden is left as it is");

    fs::create_dir_all(&output).unwrap();
    goldens.check("g", &near, OnMismatch::Fail);
    assert!(
        !output.exists(),
        "a pass clears an earlier failure's output"
    );

    goldens.check("g", &far, OnMismatch::Rewrite);
    assert_eq!(stored(), far, "a failing golden is rewritten by an update");

    panic_probe::assert_panics_with("`g` does not match its golden", || {
        goldens.check("g", &base, OnMismatch::Fail);
    });
    assert!(
        output.join("actual.png").exists(),
        "and leaves its artifacts"
    );
    assert_eq!(stored(), far, "and keeps the golden");

    panic_probe::assert_panics_with("no golden for `h`", || {
        goldens.check("h", &base, OnMismatch::Fail);
    });
    assert!(goldens.golden_path("h").exists(), "after it is written");

    let pair = dir.0.join("output").join("pair");
    fs::create_dir_all(&pair).unwrap();
    goldens.assert_same("pair", &near, &base);
    assert!(!pair.exists(), "a pair that passes clears its old output");
    panic_probe::assert_panics_with("differing pixels  16", || {
        goldens.assert_same("pair", &far, &base);
    });
    for (file, written) in [("actual.png", &far), ("expected.png", &base)] {
        assert_eq!(load(&pair.join(file)), *written, "{file}");
    }
    assert!(pair.join("diff.png").exists(), "and the diff map");
    assert!(
        !goldens.golden_path("pair").exists(),
        "comparing two images writes no golden"
    );
}

/// Adapter sidecar: first run records it, the same adapter compares normally, another adapter fails before any pixel diff, an update run adopts it.
#[test]
fn goldens_record_and_hold_their_adapter() {
    let dir = Scratch::new("adapter");
    let image = solid(2, 2, [1, 2, 3, 255]);
    let sidecar = dir.0.join("golden").join("adapter.txt");
    let first = Goldens::new(&dir.0).with_adapter("GPU A");
    first.check("g", &image, OnMismatch::Rewrite);
    assert_eq!(fs::read_to_string(&sidecar).unwrap(), "GPU A");
    first.check("g", &image, OnMismatch::Fail);

    let other = Goldens::new(&dir.0).with_adapter("GPU B");
    panic_probe::assert_panics_with("written on adapter \"GPU A\"", || {
        other.check("g", &image, OnMismatch::Fail);
    });
    other.check("g", &image, OnMismatch::Rewrite);
    assert_eq!(
        fs::read_to_string(&sidecar).unwrap(),
        "GPU B",
        "an update adopts it"
    );

    Goldens::new(&dir.0).check("g", &image, OnMismatch::Fail);

    // Tests run in parallel: a reader sees the whole adapter or no file, never a partial write.
    let names: Vec<String> = (0..8).map(|i| format!("t{i}")).collect();
    let racing = Goldens::new(&dir.0).with_adapter("GPU C");
    for name in &names {
        racing.check(name, &image, OnMismatch::Rewrite);
    }
    let sidecar_dir = sidecar.parent().unwrap();
    for _ in 0..4 {
        fs::remove_file(&sidecar).unwrap();
        let start = Barrier::new(names.len());
        thread::scope(|scope| {
            for name in &names {
                scope.spawn(|| {
                    start.wait();
                    racing.check(name, &image, OnMismatch::Fail);
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

/// Every golden no name claims is an orphan, in name order; the sidecar is not a golden. A missing
/// directory holds none; one that cannot be read panics with its path.
#[test]
fn orphans_are_the_goldens_no_name_claims() {
    let dir = Scratch::new("orphans");
    let goldens = Goldens::new(&dir.0).with_adapter("GPU A");
    assert!(goldens.orphans([]).is_empty(), "no directory, no orphans");
    let image = solid(1, 1, [0, 0, 0, 255]);
    for name in ["kept", "old", "older"] {
        goldens.check(name, &image, OnMismatch::Rewrite);
    }
    assert_eq!(
        goldens.orphans(["kept"]),
        [goldens.golden_path("old"), goldens.golden_path("older")],
    );
    assert!(goldens.orphans(["kept", "old", "older"]).is_empty());

    // A golden path that is no directory is a read error, not an empty suite.
    let blocked = Scratch::new("orphans-blocked");
    fs::create_dir_all(&blocked.0).unwrap();
    let golden = blocked.0.join("golden");
    fs::write(&golden, "").unwrap();
    panic_probe::assert_panics_with(&format!("read {}", golden.display()), || {
        Goldens::new(&blocked.0).orphans([])
    });
}
