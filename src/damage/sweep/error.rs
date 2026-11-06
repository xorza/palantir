//! What can stop a sweep before or after it measures.

use std::path::PathBuf;
use std::{error, fmt, io};

/// A sweep's command line that does not parse, or a report it cannot write.
#[derive(Debug)]
pub(super) enum SweepError {
    UnknownFlag(String),
    SecondFilter(String),
    MissingValue(&'static str),
    NotACount { flag: &'static str, value: String },
    Stdout(io::Error),
    Write { path: PathBuf, source: io::Error },
}

impl fmt::Display for SweepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFlag(flag) => write!(f, "unknown flag {flag}"),
            Self::SecondFilter(filter) => write!(f, "a second filter, {filter}"),
            Self::MissingValue(flag) => write!(f, "{flag} takes a value"),
            Self::NotACount { flag, value } => {
                write!(f, "{flag} takes a count above 0, not {value}")
            }
            Self::Stdout(_) => write!(f, "cannot write the report"),
            Self::Write { path, .. } => write!(f, "cannot write {}", path.display()),
        }
    }
}

impl From<io::Error> for SweepError {
    fn from(source: io::Error) -> Self {
        Self::Stdout(source)
    }
}

impl error::Error for SweepError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Stdout(source) | Self::Write { source, .. } => Some(source),
            Self::UnknownFlag(_)
            | Self::SecondFilter(_)
            | Self::MissingValue(_)
            | Self::NotACount { .. } => None,
        }
    }
}
