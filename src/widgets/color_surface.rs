//! A CPU-built texture a colour widget paints with, and when to rebuild it.

use crate::primitives::geometry::size::Size;
use crate::primitives::paint::image::Image;
use crate::renderer::image_registry::image_handle::ImageHandle;
use crate::ui::Ui;
use glam::UVec2;
use std::num::NonZeroU32;

/// One CPU-filled texture a colour widget owns and rewrites in place via [`ImageHandle::update`].
///
/// Exact per texel, which a gradient (linear-light interpolation) or vertex-coloured mesh (8-bit
/// *linear*, crushing darks) cannot be. `K` is everything the fill reads, compared exactly.
#[derive(Debug)]
pub(crate) struct ColorSurface<K> {
    built: Option<Built<K>>,
}

#[derive(Debug)]
struct Built<K> {
    handle: ImageHandle,
    image: Image,
    key: K,
}

/// Smallest texture either axis is built at: two texels still interpolate.
const MIN_TEXELS: u32 = 2;

/// Default texel size: how far below display resolution a surface is built; see
/// [`ColorField::texel_size`](crate::ColorField::texel_size) for the measurement behind four.
pub(crate) const TEXEL_SIZE: u32 = 4;

pub(crate) const MAX_TEXEL_SIZE: u32 = 16;

/// Texel dimensions for a surface of `size` logical px at one texel per `texel_size` physical px, under
/// the device cap. Total over every input (NaN, negative, absurd): it lands on the floor or cap.
#[expect(
    clippy::cast_sign_loss,
    reason = "the saturating cast is the clamp: a negative or NaN size lands on zero, then on the floor"
)]
pub(crate) fn texture_size(size: Size, texel_size: u32, ui: &Ui) -> UVec2 {
    let scale = ui.display().scale_factor();
    let cap = ui.max_image_dimension().map_or(u32::MAX, NonZeroU32::get);
    let floor = MIN_TEXELS.min(cap);
    let axis = |logical: f32| {
        let texels = (logical * scale / texel_size as f32).ceil();
        (texels as u32).clamp(floor, cap)
    };
    UVec2::new(axis(size.w), axis(size.h))
}

impl<K> Default for ColorSurface<K> {
    fn default() -> Self {
        Self { built: None }
    }
}

impl<K: PartialEq> ColorSurface<K> {
    /// The handle to paint with, refilled first when `size` or `key` moved. `fill` writes every texel
    /// **sRGB-encoded**: `.into()` from an `RgbaF32` is the exact encode.
    pub(crate) fn ensure(
        &mut self,
        ui: &Ui,
        size: UVec2,
        key: K,
        fill: impl FnOnce(&mut Image),
    ) -> &ImageHandle {
        if let Some(built) = self
            .built
            .as_mut()
            .filter(|built| built.image.size() == size)
        {
            if built.key != key {
                fill(&mut built.image);
                built.handle.update(&built.image);
                built.key = key;
            }
        } else {
            let mut image = Image::blank(size);
            fill(&mut image);
            let handle = ui
                .load_image(&image)
                .expect("a colour surface is clamped to the device texture cap");
            self.built = Some(Built { handle, image, key });
        }
        &self.built.as_ref().unwrap().handle
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use super::*;

    impl<K> ColorSurface<K> {
        pub(crate) fn built_size(&self) -> Option<UVec2> {
            self.built.as_ref().map(|built| built.image.size())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internals::harness::UiHarness;
    use crate::primitives::paint::color::srgba_u8::SrgbaU8;

    #[test]
    fn cached_surface_reuses_pixels_and_handle_until_resize() {
        let mut h = UiHarness::new(UVec2::new(8, 8));
        let mut surface = ColorSurface::default();
        let red = SrgbaU8::rgb(255, 0, 0);
        let blue = SrgbaU8::rgb(0, 0, 255);
        let first = surface
            .ensure(h.ui(), UVec2::new(2, 3), 1, |image| {
                image.texels_mut().fill(red);
            })
            .clone();
        let pixels = surface.built.as_ref().unwrap().image.texels().as_ptr();
        assert_eq!(first.generation(), 0);
        let reused = surface.ensure(h.ui(), UVec2::new(2, 3), 1, |_| {
            panic!("an unchanged surface must not refill");
        });
        assert_eq!(reused.id(), first.id());
        assert_eq!(reused.generation(), 0);

        let updated = surface.ensure(h.ui(), UVec2::new(2, 3), 2, |image| {
            assert_eq!(image.texels().as_ptr(), pixels);
            image.texels_mut().fill(blue);
        });
        assert_eq!(updated.id(), first.id());
        assert_eq!(first.generation(), 1);
        assert_eq!(surface.built.as_ref().unwrap().image.texels(), &[blue; 6]);

        let resized = surface.ensure(h.ui(), UVec2::new(3, 2), 2, |image| {
            assert_eq!(image.size(), UVec2::new(3, 2));
            image.texels_mut().fill(red);
        });
        assert_ne!(resized.id(), first.id());
        assert_eq!(resized.size(), UVec2::new(3, 2));
        assert_eq!(resized.generation(), 0);
        assert_eq!(surface.built.as_ref().unwrap().image.texels(), &[red; 6]);
    }
}
