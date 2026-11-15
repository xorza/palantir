//! `--against DIR`: each shot compared with the picture of the same name an earlier run wrote,
//! at zero tolerance, so a change shows as the pixels it moved.

use std::io::{self, ErrorKind, Write};
use std::path::Path;

use palantir::Image;
use palantir::glam::UVec2;
use palantir::golden::Tolerance;

use crate::shot::{ShotError, read_png, write_png};

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
/// differs. A missing earlier picture is [`Comparison::Missing`]; one that does not read is
/// [`ShotError::Read`], and a difference that does not write [`ShotError::Write`], each naming
/// its own file.
pub(crate) fn compare(
    picture: &Image,
    earlier: &Path,
    diff: &Path,
) -> Result<Comparison, ShotError> {
    let earlier = match read_png(earlier) {
        Ok(earlier) => earlier,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(Comparison::Missing),
        Err(source) => {
            return Err(ShotError::Read {
                path: earlier.to_path_buf(),
                source,
            });
        }
    };
    if earlier.size() != picture.size() {
        return Ok(Comparison::OtherSize {
            earlier: earlier.size(),
        });
    }
    let report = Tolerance::EXACT.diff(picture, &earlier);
    if report.differing_pixels > 0 {
        write_png(&report.diff_image, diff).map_err(|source| ShotError::Write {
            path: diff.to_path_buf(),
            source,
        })?;
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
