//! Why SVG sources cannot make an [`IconTable`](crate::IconTable).

use resvg::usvg;
use std::borrow::Cow;
use std::error;
use std::fmt::{self, Display, Formatter};

/// Why [`IconTable::from_svgs`](crate::IconTable::from_svgs) refused its sources; icon files are data, so each flaw is an error.
#[derive(Debug)]
pub enum IconTableError {
    /// A source does not parse as an SVG.
    Unreadable {
        /// The icon whose source failed.
        name: Cow<'static, str>,
        /// What the SVG parser reported.
        source: SvgParseError,
    },
    /// More than 65 536 sources, the most one set indexes.
    TooMany {
        /// How many sources there were.
        count: usize,
    },
    /// Two sources share a name, which [`IconSet::by_name`](crate::IconSet::by_name) could resolve to either.
    DuplicateName {
        /// The shared name.
        name: Cow<'static, str>,
    },
    /// The sources total more than `u32::MAX` bytes, past where an
    /// [`IconDefinition::svg`](crate::IconDefinition::svg) span can end.
    TooLarge {
        /// The sources' total length; `u128` holds any total of up to 65 536 sources.
        bytes: u128,
    },
}

impl Display for IconTableError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { name, source } => {
                write!(f, "icon {name:?} is not a readable SVG: {source}")
            }
            Self::TooMany { count } => {
                write!(f, "an icon set holds at most 65536 icons, got {count}")
            }
            Self::DuplicateName { name } => {
                write!(f, "two icons in one set are named {name:?}")
            }
            Self::TooLarge { bytes } => write!(
                f,
                "an icon set holds at most {} bytes of SVG, got {bytes}",
                u32::MAX
            ),
        }
    }
}

impl error::Error for IconTableError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Unreadable { source, .. } => Some(source),
            Self::TooMany { .. } | Self::DuplicateName { .. } | Self::TooLarge { .. } => None,
        }
    }
}

/// An SVG parser failure kept whole but unnamed, so no parser type reaches the published surface.
#[derive(Debug)]
pub struct SvgParseError(usvg::Error);

impl SvgParseError {
    pub(crate) const fn new(error: usvg::Error) -> Self {
        Self(error)
    }
}

impl Display for SvgParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl error::Error for SvgParseError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.0.source()
    }
}
