//! [`Build`]: one feature set palantir is documented under.

use std::fmt;

/// One feature set palantir is documented under: none, or one public feature alone. The surface
/// is the union over every build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Build {
    NoDefault,
    Feature(String),
}

impl fmt::Display for Build {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDefault => f.write_str("no-default"),
            Self::Feature(feature) => f.write_str(feature),
        }
    }
}
