//! Why SVG sources cannot make an [`IconTable`](crate::IconTable).

use std::borrow::Cow;
use std::error;
use std::fmt::{self, Display, Formatter};

/// Why [`IconTable::from_svgs`](crate::IconTable::from_svgs) refused its sources; icon files are data, so each flaw is an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconTableError {
    /// A source does not parse as an SVG.
    Unreadable {
        /// The icon whose source failed.
        name: Cow<'static, str>,
    },
    /// More sources than an [`IconId`](crate::IconId) can name.
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
            Self::Unreadable { name } => write!(f, "icon {name:?} is not a readable SVG"),
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

impl error::Error for IconTableError {}
