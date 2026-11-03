//! Failures the text system reports.

use std::error::Error;
use std::fmt;
use std::fmt::{Display, Formatter};

/// A font could not be registered; a `Result` as its bytes are untrusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontLoadError {
    /// The bytes parsed to no usable face.
    NoFaces,
    /// Every family the font names is longer than
    /// [`FontFamily::MAX_NAME_LEN`](crate::FontFamily::MAX_NAME_LEN) bytes.
    FamilyNameTooLong,
}

impl Display for FontLoadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFaces => f.write_str("the font data holds no usable face"),
            Self::FamilyNameTooLong => {
                f.write_str("every family the font names is longer than a font family name holds")
            }
        }
    }
}

impl Error for FontLoadError {}
