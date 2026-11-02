//! Why a range makes no [`Span`](crate::Span).

use std::error;
use std::fmt::{self, Display, Formatter};

/// Why [`Span`](crate::Span)'s `TryFrom` refused a range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanRangeError {
    /// The range ends before it starts.
    Backward {
        /// Where the range starts.
        start: usize,
        /// Where the range ends, before `start`.
        end: usize,
    },
    /// The range ends past `u32::MAX`, where a span's indices stop.
    PastU32 {
        /// Where the range ends.
        end: usize,
    },
}

impl Display for SpanRangeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Backward { start, end } => {
                write!(f, "the range {start}..{end} ends before it starts")
            }
            Self::PastU32 { end } => write!(f, "the range ends at {end}, past u32::MAX"),
        }
    }
}

impl error::Error for SpanRangeError {}
