//! Failures the text system reports.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

/// A font could not be registered.
///
/// A `Result` rather than an assert because both arms are untrusted
/// input: a path the app builds at runtime, and bytes that may not be a
/// font at all.
#[derive(Debug)]
pub enum FontLoadError {
    /// The file could not be read or memory-mapped.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The bytes parsed to no usable face.
    NoFaces,
}

impl Display for FontLoadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "cannot read the font file {}: {source}", path.display())
            }
            Self::NoFaces => f.write_str("the font data holds no usable face"),
        }
    }
}

impl Error for FontLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::NoFaces => None,
        }
    }
}
