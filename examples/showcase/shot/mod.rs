//! `showcase --shot SCRIPT [--out DIR]`: the showcase played from a script on a window with no
//! desktop, its pictures written as PNGs and a report of each step printed, for an agent that
//! checks a change by looking at it. One process a script, so a run never shows a stale build.

#[cfg(feature = "golden")]
mod against;
mod report;
pub(crate) mod script;

use std::error;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use palantir::glam::UVec2;
use palantir::{Gpu, Harness, Image, PlayError, Rect};

use crate::shell::{self, Shell, UnknownPage};
use crate::shot::report::Legend;
use crate::shot::script::{ParseError, Region, Script, StepKind};

/// Where pictures go when the call names no folder.
pub(crate) const DEFAULT_OUT: &str = "target/shot";

/// The folders a run writes into and compares against.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Folders<'a> {
    /// Where pictures go.
    pub(crate) out: &'a Path,
    /// Where an earlier run's pictures are, each shot compared with the one of its name. Only a
    /// build with the `golden` feature compares.
    pub(crate) against: Option<&'a Path>,
}

/// Why a script stopped.
#[derive(Debug)]
pub(crate) enum ShotError {
    /// The script file does not read.
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse(ParseError),
    Page(UnknownPage),
    /// A step failed in the window: its line, 0 for the launch, and why.
    Play {
        line: usize,
        error: PlayError,
    },
    /// A zoom that would make a picture past the largest side.
    TooLarge {
        line: usize,
        side: u32,
        max_side: u32,
    },
    /// A picture does not write.
    Write {
        path: PathBuf,
        source: io::Error,
    },
    /// The report does not write.
    Report(io::Error),
}

impl Display for ShotError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Parse(error) => error.fmt(f),
            Self::Page(error) => error.fmt(f),
            Self::Play { line: 0, error } => write!(f, "the launch: {error}"),
            Self::Play { line, error } => write!(f, "line {line}: {error}"),
            Self::TooLarge {
                line,
                side,
                max_side,
            } => write!(
                f,
                "line {line}: the zoomed picture is {side} px a side, past max-side {max_side}; \
                 zoom less or take a smaller region"
            ),
            Self::Write { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Report(source) => write!(f, "the report: {source}"),
        }
    }
}

impl error::Error for ShotError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } | Self::Report(source) => {
                Some(source)
            }
            Self::Parse(error) => Some(error),
            Self::Page(error) => Some(error),
            Self::Play { error, .. } => Some(error),
            Self::TooLarge { .. } => None,
        }
    }
}

impl From<ParseError> for ShotError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl From<UnknownPage> for ShotError {
    fn from(error: UnknownPage) -> Self {
        Self::Page(error)
    }
}

/// Play the script at `script` on `gpu`, writing pictures into `folders` and the report into
/// `report`.
pub(crate) fn run(
    gpu: Gpu,
    script: &Path,
    folders: Folders<'_>,
    report: &mut impl Write,
) -> Result<(), ShotError> {
    let text = fs::read_to_string(script).map_err(|source| ShotError::Read {
        path: script.to_path_buf(),
        source,
    })?;
    play(gpu, &Script::parse(&text)?, folders, report)
}

/// Play `script` on `gpu`; see [`run`].
pub(crate) fn play(
    gpu: Gpu,
    script: &Script,
    folders: Folders<'_>,
    report: &mut impl Write,
) -> Result<(), ShotError> {
    let out = folders.out;
    let header = &script.header;
    let mut shell = Shell::with_page(header.page.as_deref())?;
    let mut h = Harness::builder(header.size)
        .scale(header.scale)
        .gpu(gpu)
        .build();
    shell::pin_dark(h.ui());
    let failed = |line| move |error| ShotError::Play { line, error };
    h.settle(&mut shell).map_err(failed(0))?;
    let written = |path: &Path| {
        let path = path.to_path_buf();
        move |source| ShotError::Write { path, source }
    };
    for step in &script.steps {
        let line = step.line;
        match &step.kind {
            StepKind::Act(action) => {
                h.play(&mut shell, std::slice::from_ref(action))
                    .map_err(|error| failed(line)(error.error))?;
            }
            StepKind::Shot { name, region, zoom } => {
                let region = match region {
                    None => None,
                    Some(Region::Rect(rect)) => Some(*rect),
                    Some(Region::At(at)) => Some(
                        h.find(&shell, at)
                            .map_err(|error| failed(line)(error.into()))?
                            .rect,
                    ),
                };
                let shot = h.shot(&mut shell, region).map_err(failed(line))?;
                let picture = if *zoom > 1 {
                    let side = shot.image().size().max_element() * zoom;
                    if side > header.max_side {
                        return Err(ShotError::TooLarge {
                            line,
                            side,
                            max_side: header.max_side,
                        });
                    }
                    magnified(shot.image(), *zoom)
                } else {
                    shot.image().fitted(header.max_side)
                };
                fs::create_dir_all(out).map_err(written(out))?;
                let path = out.join(format!("{name}.png"));
                write_png(&picture, &path).map_err(written(&path))?;
                let widgets = h.widgets(None);
                let legend = Legend::of(&shot, picture.size(), &widgets);
                report::shot(report, &path, &shot, picture.size(), &legend)
                    .map_err(ShotError::Report)?;
                if let Some(against) = folders.against {
                    compare(
                        &picture,
                        &against.join(format!("{name}.png")),
                        &out.join(format!("{name}.diff.png")),
                        report,
                    )?;
                }
            }
            StepKind::Probe(point) => {
                let pixel = Rect::new(point.x, point.y, 1.0, 1.0);
                let shot = h.shot(&mut shell, Some(pixel)).map_err(failed(line))?;
                let image = shot.image();
                let center = image.size() / 2;
                let texel = image.texels()[(center.y * image.size().x + center.x) as usize];
                report::probe(report, *point, texel).map_err(ShotError::Report)?;
            }
            StepKind::Widgets(filter) => {
                let widgets = h.widgets(filter.as_deref());
                report::widgets(report, &widgets).map_err(ShotError::Report)?;
            }
        }
    }
    Ok(())
}

/// `image` magnified `zoom` times, each texel a `zoom` × `zoom` block.
fn magnified(image: &Image, zoom: u32) -> Image {
    let size = image.size();
    let mut out = Image::blank(size * zoom);
    let texels = image.texels();
    out.fill_with(|at: UVec2| {
        let from = at / zoom;
        texels[(from.y * size.x + from.x) as usize]
    });
    out
}

/// Compare `picture` with the earlier one at `earlier`, the difference marked in `diff`, and
/// report how they compare.
#[cfg(feature = "golden")]
fn compare(
    picture: &Image,
    earlier: &Path,
    diff: &Path,
    report: &mut impl Write,
) -> Result<(), ShotError> {
    let comparison =
        against::compare(picture, earlier, diff).map_err(|source| ShotError::Write {
            path: earlier.to_path_buf(),
            source,
        })?;
    against::report(report, earlier, &comparison, diff).map_err(ShotError::Report)
}

/// A build without the `golden` feature has no comparison; `main` refuses `--against` there.
#[cfg(not(feature = "golden"))]
fn compare(
    _picture: &Image,
    _earlier: &Path,
    _diff: &Path,
    _report: &mut impl Write,
) -> Result<(), ShotError> {
    unreachable!("main refuses --against without the golden feature")
}

/// `path`'s PNG, as RGBA bytes.
#[cfg(feature = "golden")]
fn read_png(path: &Path) -> io::Result<Image> {
    let decoded = image::open(path).map_err(io::Error::other)?.into_rgba8();
    let size = UVec2::new(decoded.width(), decoded.height());
    Image::from_srgba8(size, decoded.into_raw()).map_err(io::Error::other)
}

fn write_png(image: &Image, path: &Path) -> io::Result<()> {
    let size = image.size();
    let bytes: Vec<u8> = image
        .texels()
        .iter()
        .flat_map(|texel| [texel.r, texel.g, texel.b, texel.a])
        .collect();
    let buffer = image::RgbaImage::from_raw(size.x, size.y, bytes)
        .expect("an image holds four bytes a texel");
    buffer
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(io::Error::other)
}

#[cfg(test)]
mod tests;
