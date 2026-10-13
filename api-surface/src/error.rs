//! [`Error`]: why the surface could not be written.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

/// A step the surface depends on that failed.
#[derive(Debug)]
pub(crate) enum Error {
    /// `cargo` could not be started.
    Spawn(io::Error),
    /// A `cargo` call ran and failed; its own output says why.
    Cargo {
        command: &'static str,
        status: ExitStatus,
    },
    /// `cargo metadata` named no package or field the surface needs.
    Metadata(&'static str),
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Write {
        path: PathBuf,
        source: io::Error,
    },
    Remove {
        path: PathBuf,
        source: io::Error,
    },
    Json(serde_json::Error),
    /// The nightly wrote a rustdoc JSON format other than the one `rustdoc-types` reads.
    FormatVersion {
        found: Option<u64>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(_) => write!(f, "could not run cargo"),
            Self::Cargo { command, status } => write!(f, "`{command}` failed: {status}"),
            Self::Metadata(missing) => write!(f, "cargo metadata has no {missing}"),
            Self::Read { path, .. } => write!(f, "could not read {}", path.display()),
            Self::Write { path, .. } => write!(f, "could not write {}", path.display()),
            Self::Remove { path, .. } => write!(f, "could not remove {}", path.display()),
            Self::Json(_) => write!(f, "rustdoc or cargo wrote JSON this tool cannot read"),
            Self::FormatVersion { found } => {
                let found = found.map_or_else(|| "no".to_owned(), |v| v.to_string());
                write!(
                    f,
                    "the nightly wrote rustdoc JSON format {found}, and this renderer reads {}: \
                     bump `rustdoc-types` to the release for the new format, then check the \
                     renderer against its changes",
                    rustdoc_types::FORMAT_VERSION
                )
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(source)
            | Self::Read { source, .. }
            | Self::Write { source, .. }
            | Self::Remove { source, .. } => Some(source),
            Self::Json(source) => Some(source),
            Self::Cargo { .. } | Self::Metadata(_) | Self::FormatVersion { .. } => None,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(source: serde_json::Error) -> Self {
        Self::Json(source)
    }
}
