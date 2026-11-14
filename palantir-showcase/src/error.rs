//! [`RunError`]: why the tour did not run as the command line asked.

use std::error;
use std::fmt::{self, Display, Formatter};

use palantir::{GpuRequestError, WinitHostError};

use crate::shot::{ShotError, UnknownPage};

/// Why the tour did not run, printed once through [`ErrorChain`](palantir::ErrorChain).
#[derive(Debug)]
pub(crate) enum RunError {
    /// The arguments are not the tour's.
    Usage,
    /// `--page` names a page the tour lacks.
    UnknownPage(UnknownPage),
    /// `--against` compares, which a build without the `golden` feature cannot.
    AgainstNeedsGolden,
    /// No headless device opened for a script.
    Gpu(GpuRequestError),
    /// The window did not open or run.
    Window(WinitHostError),
    /// A script stopped.
    Shot(ShotError),
}

impl Display for RunError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => write!(
                f,
                "usage: {} [--page PAGE | --shot SCRIPT [--out DIR] [--against DIR]]",
                env!("CARGO_CRATE_NAME")
            ),
            Self::UnknownPage(error) => error.fmt(f),
            Self::AgainstNeedsGolden => {
                f.write_str("--against compares through the golden feature; build with it")
            }
            Self::Gpu(_) => f.write_str("no headless graphics device opened"),
            Self::Window(_) => f.write_str("the window did not run"),
            Self::Shot(error) => error.fmt(f),
        }
    }
}

impl error::Error for RunError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Usage | Self::AgainstNeedsGolden => None,
            // Transparent: the `Display` is their own words.
            Self::UnknownPage(error) => error.source(),
            Self::Shot(error) => error.source(),
            Self::Gpu(source) => Some(source),
            Self::Window(source) => Some(source),
        }
    }
}

impl From<UnknownPage> for RunError {
    fn from(error: UnknownPage) -> Self {
        Self::UnknownPage(error)
    }
}

impl From<GpuRequestError> for RunError {
    fn from(error: GpuRequestError) -> Self {
        Self::Gpu(error)
    }
}

impl From<WinitHostError> for RunError {
    fn from(error: WinitHostError) -> Self {
        Self::Window(error)
    }
}

impl From<ShotError> for RunError {
    fn from(error: ShotError) -> Self {
        Self::Shot(error)
    }
}
