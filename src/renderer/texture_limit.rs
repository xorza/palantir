//! The device ceiling every texture is measured against.

use crate::renderer::error::ImageTooLarge;
use glam::UVec2;
use std::num::NonZeroU32;

/// The device's `max_texture_dimension_2d`, the one ceiling for any texture palantir allocates or accepts.
///
/// The gradient atlas, image registration and
/// [`Ui::max_image_dimension`](crate::Ui::max_image_dimension) share it as one type. `None` is a standalone CPU recorder: no device, no ceiling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextureLimit(Option<NonZeroU32>);

impl TextureLimit {
    /// The ceiling a device granted at creation; see `Gpu::max_texture_dim`.
    pub(crate) const fn from_device(max_dimension: NonZeroU32) -> Self {
        Self(Some(max_dimension))
    }

    /// The largest width or height accepted, or `None` with no device.
    pub(crate) const fn max_dimension(self) -> Option<NonZeroU32> {
        self.0
    }

    /// Reject `size` when either axis exceeds the ceiling; never shrinks, since scaling is the caller's call.
    pub(crate) fn accepts(self, size: UVec2) -> Result<(), ImageTooLarge> {
        match self.0.map(NonZeroU32::get) {
            Some(max_dimension) if size.x > max_dimension || size.y > max_dimension => {
                Err(ImageTooLarge {
                    size,
                    max_dimension,
                })
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::renderer::error::ImageTooLarge;
    use crate::renderer::texture_limit::TextureLimit;
    use glam::UVec2;
    use std::num::NonZeroU32;

    /// The accessor reports exactly what the check enforces.
    #[test]
    fn the_reported_ceiling_is_the_one_enforced() {
        let limit = TextureLimit::from_device(NonZeroU32::new(4).unwrap());
        assert_eq!(limit.max_dimension(), NonZeroU32::new(4));
        assert_eq!(limit.accepts(UVec2::new(4, 4)), Ok(()));
        for size in [UVec2::new(5, 1), UVec2::new(1, 5)] {
            assert_eq!(
                limit.accepts(size),
                Err(ImageTooLarge {
                    size,
                    max_dimension: 4,
                }),
            );
        }
    }

    /// A deviceless recorder reports none.
    #[test]
    fn a_deviceless_limit_accepts_any_size() {
        let limit = TextureLimit::default();
        assert_eq!(limit.max_dimension(), None);
        assert_eq!(
            limit.accepts(UVec2::new(u32::from(u16::MAX) + 1, 1)),
            Ok(())
        );
    }
}
