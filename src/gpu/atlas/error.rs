//! What the pool refuses.

use glam::UVec2;
use std::fmt::{self, Display};

/// Why an item found no room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AtlasError {
    /// Wider or taller than a page, or for scratch than the device's edge: no eviction makes room.
    Oversized { size: UVec2, edge: u32 },
    /// Every page is open at its edge and every item on it was drawn this frame or the last, or is
    /// pinned: room may come back as the frames move on.
    Full,
}

impl Display for AtlasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Oversized { size, edge } => {
                write!(
                    f,
                    "an item of {}×{} passes the edge of {edge}",
                    size.x, size.y
                )
            }
            Self::Full => f.write_str("the atlas holds no room an eviction can make"),
        }
    }
}

impl std::error::Error for AtlasError {}
