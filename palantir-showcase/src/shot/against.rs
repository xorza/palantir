//! `--against DIR`: each shot compared with the picture of the same name an earlier run wrote,
//! at zero tolerance, so a change shows as the pixels it moved.

use std::io::{self, Write};
use std::path::Path;

use palantir::Image;
use palantir::glam::UVec2;
use palantir::golden::Tolerance;

use crate::shot::{read_png, write_png};

/// How a shot compares with the earlier picture of its name.
#[derive(Debug, PartialEq)]
pub(crate) enum Comparison {
    /// The earlier run wrote no picture of this name.
    Missing,
    /// The earlier picture has another size, which no pixel compares across.
    OtherSize { earlier: UVec2 },
    /// The pixels that differ, and the largest difference of one channel; 0 and 0 for the same
    /// picture.
    Diff { pixels: u32, max_delta: u8 },
}

/// Compare `picture` with `earlier`'s PNG, writing the marked difference to `diff` when a pixel
/// differs.
pub(crate) fn compare(picture: &Image, earlier: &Path, diff: &Path) -> io::Result<Comparison> {
    if !earlier.exists() {
        return Ok(Comparison::Missing);
    }
    let earlier = read_png(earlier)?;
    if earlier.size() != picture.size() {
        return Ok(Comparison::OtherSize {
            earlier: earlier.size(),
        });
    }
    let report = Tolerance::EXACT.diff(picture, &earlier);
    if report.differing_pixels > 0 {
        write_png(&report.diff_image, diff)?;
    }
    Ok(Comparison::Diff {
        pixels: report.differing_pixels,
        max_delta: report.max_channel_delta,
    })
}

/// The comparison's line of a shot's report.
pub(crate) fn report(
    out: &mut impl Write,
    earlier: &Path,
    comparison: &Comparison,
    diff: &Path,
) -> io::Result<()> {
    match comparison {
        Comparison::Missing => writeln!(out, "  against {}: missing", earlier.display()),
        Comparison::OtherSize { earlier: size } => writeln!(
            out,
            "  against {}: {}x{}, another size",
            earlier.display(),
            size.x,
            size.y
        ),
        Comparison::Diff {
            pixels: 0,
            max_delta: _,
        } => writeln!(out, "  against {}: the same", earlier.display()),
        Comparison::Diff { pixels, max_delta } => writeln!(
            out,
            "  against {}: {pixels} pixels differ, by at most {max_delta}; marked in {}",
            earlier.display(),
            diff.display()
        ),
    }
}
