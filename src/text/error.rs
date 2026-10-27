//! Failures the text system reports.

use std::error::Error;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::io;
use std::path::PathBuf;

/// A font could not be registered; a `Result` as path and bytes are untrusted.
#[derive(Debug)]
pub enum FontLoadError {
    /// The file could not be read or memory-mapped.
    Io {
        /// The file that could not be read.
        path: PathBuf,
        /// What the filesystem reported.
        source: io::Error,
    },
    /// The bytes parsed to no usable face.
    NoFaces,
    /// Every family the font names is longer than
    /// [`FontFamily::MAX_NAME_LEN`](crate::FontFamily::MAX_NAME_LEN) bytes.
    FamilyNameTooLong,
}

impl Display for FontLoadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "cannot read the font file {}: {source}", path.display())
            }
            Self::NoFaces => f.write_str("the font data holds no usable face"),
            Self::FamilyNameTooLong => {
                f.write_str("every family the font names is longer than a font family name holds")
            }
        }
    }
}

impl Error for FontLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::NoFaces | Self::FamilyNameTooLong => None,
        }
    }
}
