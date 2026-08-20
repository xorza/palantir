//! The device ceiling every texture this crate touches is measured
//! against, and what exceeding it reports.

use glam::UVec2;
use std::fmt::{Display, Formatter};
use std::num::NonZeroU32;

/// The selected device's `max_texture_dimension_2d` — the single ceiling
/// palantir imposes on any texture it allocates or accepts.
///
/// **One value, threaded as one thing.** The gradient atlas caps its row
/// count under it, image registration rejects a source past it, and
/// [`Ui::max_image_dimension`](crate::Ui::max_image_dimension) reports it
/// so a caller can size a downscale against the device actually in use.
/// Those all used to receive a bare `Option<NonZeroU32>` from the same
/// call site, with nothing but the argument name saying they were the same
/// number — and the enforcement lived inside the image registry, whose job
/// is the upload/release lifecycle and not this.
///
/// `None` is a standalone CPU recorder: no device to ask, and so no
/// ceiling to enforce.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextureLimit(Option<NonZeroU32>);

impl TextureLimit {
    /// The ceiling a device granted at creation — see
    /// `DeviceRequirements::max_texture_dim`.
    pub(crate) fn from_device(max_dimension: NonZeroU32) -> Self {
        Self(Some(max_dimension))
    }

    /// The largest width or height this limit accepts, or `None` where
    /// there is no device and so no ceiling.
    pub(crate) fn max_dimension(self) -> Option<NonZeroU32> {
        self.0
    }

    /// Reject `size` when either axis exceeds the ceiling.
    ///
    /// Rejects rather than shrinks: a caller that wants the biggest
    /// texture a machine will take asks [`Self::max_dimension`] first and
    /// scales its source, which is a decision only it can make.
    pub(crate) fn accepts(self, size: UVec2) -> Result<(), RegisterImageError> {
        match self.0.map(NonZeroU32::get) {
            Some(max_dimension) if size.x > max_dimension || size.y > max_dimension => {
                Err(RegisterImageError {
                    size,
                    max_dimension,
                })
            }
            _ => Ok(()),
        }
    }
}

/// Why an [`Image`](crate::primitives::image::Image) could not be
/// registered for GPU upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegisterImageError {
    /// Rejected intrinsic pixel dimensions.
    pub size: glam::UVec2,
    /// Maximum accepted width or height for the selected device.
    pub max_dimension: u32,
}

impl Display for RegisterImageError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "image is {}x{} px but the device's maximum 2D texture dimension is {}",
            self.size.x, self.size.y, self.max_dimension,
        )
    }
}

impl std::error::Error for RegisterImageError {}

#[cfg(test)]
mod tests {
    use crate::renderer::texture_limit::{RegisterImageError, TextureLimit};
    use glam::UVec2;
    use std::num::NonZeroU32;

    /// What the accessor reports is exactly what the check enforces — a
    /// caller sizing a downscale against it must land on the largest image
    /// that still registers, not one past it.
    #[test]
    fn the_reported_ceiling_is_the_one_enforced() {
        let limit = TextureLimit::from_device(NonZeroU32::new(4).unwrap());
        assert_eq!(limit.max_dimension(), NonZeroU32::new(4));
        assert_eq!(limit.accepts(UVec2::new(4, 4)), Ok(()));
        for size in [UVec2::new(5, 1), UVec2::new(1, 5)] {
            assert_eq!(
                limit.accepts(size),
                Err(RegisterImageError {
                    size,
                    max_dimension: 4,
                }),
            );
        }
    }

    /// A deviceless recorder reports the ceiling it enforces: none.
    #[test]
    fn a_deviceless_limit_accepts_any_size() {
        let limit = TextureLimit::default();
        assert_eq!(limit.max_dimension(), None);
        assert_eq!(limit.accepts(UVec2::new(u16::MAX as u32 + 1, 1)), Ok(()));
    }
}
