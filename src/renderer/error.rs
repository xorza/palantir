//! Failures the renderer reports.

use glam::UVec2;
use std::error;
use std::fmt;
use std::fmt::{Display, Formatter};

/// Why an [`Image`](crate::primitives::paint::image::Image) could not be loaded
/// for GPU upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageLoadError {
    /// Rejected intrinsic pixel dimensions.
    pub size: UVec2,
    /// Maximum accepted width or height for the selected device.
    pub max_dimension: u32,
}

impl Display for ImageLoadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "image is {}x{} px but the device's maximum 2D texture dimension is {}",
            self.size.x, self.size.y, self.max_dimension,
        )
    }
}

impl error::Error for ImageLoadError {}
