//! `palantir-showcase --shot SCRIPT [--out DIR] [--against DIR]`: the tour played from a script on a
//! window with no desktop, its pictures written as PNGs and a report of each step printed, for an
//! agent that checks a change by looking at it. One process a script, so a run never shows a
//! stale build.

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
use palantir::{AgentWindow, Depth, Gpu, Harness, Image, PlayError, Rect, RequestedGpu};

use crate::shell::Shell;
use crate::shot::report::Legend;
use crate::shot::script::{ParseError, Region, Script, StepKind};

/// Where pictures go when the call names no folder.
pub(crate) const DEFAULT_OUT: &str = "target/shot";

/// A page label no page has.
#[derive(Debug)]
pub(crate) struct UnknownPage {
    label: String,
    pages: Vec<&'static str>,
}

impl UnknownPage {
    /// The index of `label` among the tour's pages, ignoring case; the first page for none.
    fn find(label: Option<&str>) -> Result<usize, Self> {
        let Some(label) = label else {
            return Ok(0);
        };
        Shell::page_labels()
            .position(|page| page.eq_ignore_ascii_case(label))
            .ok_or_else(|| Self {
                label: label.to_owned(),
                pages: Shell::page_labels().collect(),
            })
    }
}

impl Display for UnknownPage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "no page \"{}\"; the pages are: ", self.label)?;
        for (idx, page) in self.pages.iter().enumerate() {
            let sep = if idx == 0 { "" } else { ", " };
            write!(f, "{sep}\"{page}\"")?;
        }
        Ok(())
    }
}

impl error::Error for UnknownPage {}

/// What the command line asks.
#[derive(Debug)]
pub(crate) enum Mode {
    /// The window, open on the page at `page`, an index into [`Shell::page_labels`].
    Window { page: usize },
    /// A script played with no desktop, its pictures written into `out` and compared with the ones
    /// in `against`.
    Shot {
        script: PathBuf,
        out: PathBuf,
        against: Option<PathBuf>,
    },
}

impl Mode {
    /// The mode `args` ask, or why they are refused: the usage, or a page the tour lacks.
    pub(crate) fn of(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let usage = || {
            format!(
                "usage: {} [--page PAGE | --shot SCRIPT [--out DIR] [--against DIR]]",
                env!("CARGO_CRATE_NAME")
            )
        };
        let Some(first) = args.next() else {
            return Ok(Self::Window { page: 0 });
        };
        if first == "--page" {
            let label = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            let page = UnknownPage::find(Some(&label)).map_err(|error| error.to_string())?;
            return Ok(Self::Window { page });
        }
        if first != "--shot" {
            return Err(usage());
        }
        let script = PathBuf::from(args.next().ok_or_else(usage)?);
        let mut out = None;
        let mut against = None;
        while let Some(flag) = args.next() {
            let slot = match flag.as_str() {
                "--out" => &mut out,
                "--against" => &mut against,
                _ => return Err(usage()),
            };
            if slot.is_some() {
                return Err(usage());
            }
            *slot = Some(PathBuf::from(args.next().ok_or_else(usage)?));
        }
        if against.is_some() && !cfg!(feature = "golden") {
            return Err("--against compares through the golden feature; build with it".to_owned());
        }
        Ok(Self::Shot {
            script,
            out: out.unwrap_or_else(|| PathBuf::from(DEFAULT_OUT)),
            against,
        })
    }
}

/// Play the script at `script` on a headless device, its report on stdout.
pub(crate) fn shots(script: &Path, folders: Folders<'_>) -> Result<(), String> {
    // No optional features: the tour draws with what every adapter has.
    #[expect(
        clippy::default_trait_access,
        reason = "the features are a wgpu type, which only `palantir::gpu` names"
    )]
    let gpu = RequestedGpu::headless(
        palantir::PowerPreference::HighPerformance,
        Default::default(),
    )
    .map_err(|error| error.to_string())?
    .gpu;
    run(gpu, script, folders, &mut io::stdout().lock()).map_err(|error| error.to_string())
}

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
    /// A file does not read: the script, or an earlier run's picture a shot compares with.
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
        /// The zoomed side, which a `u32` texel count need not hold.
        side: u64,
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
    let page = UnknownPage::find(header.page.as_deref())?;
    let mut h = Harness::builder(header.size)
        .system_scale(header.scale)
        .depth(Depth::gpu(gpu))
        .build();
    h.set_system_color_scheme(header.scheme);
    h.ui_mut().set_preferences(Shell::PREFERENCES);
    let mut shell = Shell::open(page);
    let mut window = h.session(&mut shell);
    let failed = |line| move |error| ShotError::Play { line, error };
    window.settle().map_err(failed(0))?;
    let written = |path: &Path| {
        let path = path.to_path_buf();
        move |source| ShotError::Write { path, source }
    };
    for step in &script.steps {
        let line = step.line;
        match &step.kind {
            StepKind::Act(action) => {
                window
                    .play(std::slice::from_ref(action))
                    .map_err(|error| failed(line)(error.error))?;
            }
            StepKind::Shot { name, region, zoom } => {
                let region = match region {
                    None => None,
                    Some(Region::Rect(rect)) => Some(*rect),
                    Some(Region::At(at)) => Some(window.find(at).map_err(failed(line))?.rect),
                };
                let shot = window.shot(region).map_err(failed(line))?;
                let picture = if *zoom > 1 {
                    let side = u64::from(shot.image().size().max_element()) * u64::from(*zoom);
                    if side > u64::from(header.max_side) {
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
                let widgets = window.widgets(None).map_err(failed(line))?;
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
                let shot = window.shot(Some(pixel)).map_err(failed(line))?;
                let image = shot.image();
                let center = image.size() / 2;
                let texel = image.texels()[(center.y * image.size().x + center.x) as usize];
                report::probe(report, *point, texel).map_err(ShotError::Report)?;
            }
            StepKind::Widgets(filter) => {
                let widgets = window.widgets(filter.as_deref()).map_err(failed(line))?;
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
    let comparison = against::compare(picture, earlier, diff)?;
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

/// `path`'s PNG, as RGBA bytes. A file that does not open keeps its own error, so a caller can
/// tell a missing file from a broken one.
#[cfg(feature = "golden")]
fn read_png(path: &Path) -> io::Result<Image> {
    let decoded = image::open(path)
        .map_err(|error| match error {
            image::ImageError::IoError(error) => error,
            error => io::Error::other(error),
        })?
        .into_rgba8();
    let size = UVec2::new(decoded.width(), decoded.height());
    Image::from_srgba8(size, decoded.into_raw()).map_err(io::Error::other)
}

/// `image` as a PNG at `path`, written from its bytes as they are.
fn write_png(image: &Image, path: &Path) -> io::Result<()> {
    let size = image.size();
    image::save_buffer_with_format(
        path,
        image.as_srgba8(),
        size.x,
        size.y,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(io::Error::other)
}
