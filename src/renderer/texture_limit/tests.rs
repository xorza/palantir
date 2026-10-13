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
