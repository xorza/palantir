//! Why raw pixels cannot make an [`Image`](crate::Image).

use glam::UVec2;
use std::error;
use std::fmt::{self, Display, Formatter};

/// Why [`Image::from_srgba8`](crate::Image::from_srgba8) refused its pixels.
/// Pixels usually come from a decoder or a file, so each flaw is an error
/// rather than a panic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageDataError {
    /// A dimension is zero.
    ZeroSize {
        /// The size given.
        size: UVec2,
    },
    /// The byte length of the size overflows the address space.
    TooLarge {
        /// The size given.
        size: UVec2,
    },
    /// The pixel buffer is not `size.x * size.y * 4` bytes long.
    LengthMismatch {
        /// The size given.
        size: UVec2,
        /// The byte length the size needs.
        expected: usize,
        /// The byte length given.
        actual: usize,
    },
}

impl Display for ImageDataError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            Self::ZeroSize { size } => {
                write!(
                    f,
                    "RGBA8 dimensions must be non-zero, got {}x{}",
                    size.x, size.y
                )
            }
            Self::TooLarge { size } => write!(
                f,
                "RGBA8 dimensions {}x{} overflow addressable byte length",
                size.x, size.y
            ),
            Self::LengthMismatch {
                size,
                expected,
                actual,
            } => write!(
                f,
                "RGBA8 byte length {actual} does not match {}x{}x4 = {expected}",
                size.x, size.y
            ),
        }
    }
}

impl error::Error for ImageDataError {}
