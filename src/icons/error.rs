//! Why SVG sources cannot make an [`IconTable`](crate::IconTable).

use std::borrow::Cow;
use std::error;
use std::fmt::{self, Display, Formatter};

/// Why [`IconTable::from_svgs`](crate::IconTable::from_svgs) refused its
/// sources. Icon files are data, so each flaw is an error rather than a
/// panic or a silent drop.
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
    /// Two sources share a name, which
    /// [`IconSet::by_name`](crate::IconSet::by_name) could then resolve to
    /// either.
    DuplicateName {
        /// The shared name.
        name: Cow<'static, str>,
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
        }
    }
}

impl error::Error for IconTableError {}
