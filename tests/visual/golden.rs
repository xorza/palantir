//! Golden-image workflow: compare an actual `RgbaImage` against a
//! committed PNG, auto-create on first run, dump diff artifacts on
//! failure. Set `UPDATE_GOLDEN=1` to force-rewrite an existing golden.

use std::path::{Path, PathBuf};

use image::RgbaImage;

use crate::diff::{Tolerance, diff};

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/visual/golden")
        .join(format!("{name}.png"))
}

fn output_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/visual/output")
        .join(name)
}

pub fn assert_matches_golden(name: &str, actual: &RgbaImage, tol: Tolerance) {
    let golden = golden_path(name);
    let force = std::env::var_os("UPDATE_GOLDEN").is_some_and(|v| !v.is_empty());

    if force || !golden.exists() {
        std::fs::create_dir_all(golden.parent().unwrap()).expect("mkdir golden");
        actual.save(&golden).expect("save golden");
        eprintln!(
            "{}: wrote {}",
            if force {
                "UPDATE_GOLDEN"
            } else {
                "NEW GOLDEN (no prior image)"
            },
            golden.display(),
        );
        return;
    }

    let expected = image::open(&golden)
        .unwrap_or_else(|e| panic!("read golden {}: {e}", golden.display()))
        .to_rgba8();

    let report = diff(actual, &expected, tol);
    if report.passes(tol) {
        return;
    }

    let out = output_path(name);
    std::fs::create_dir_all(&out).expect("mkdir output");
    actual.save(out.join("actual.png")).expect("save actual");
    expected
        .save(out.join("expected.png"))
        .expect("save expected");
    report
        .diff_image
        .save(out.join("diff.png"))
        .expect("save diff");

    panic!(
        "visual diff failed for `{name}`:\n  max_channel_delta = {}\n  differing_pixels  = {}\n  differing_ratio   = {:.4}\n  tolerance         = per_channel {}, max_ratio {}\n  artifacts written to {}",
        report.max_channel_delta,
        report.differing_pixels,
        report.differing_ratio,
        tol.per_channel,
        tol.max_ratio,
        out.display(),
    );
}
