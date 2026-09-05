//! A CPU-built texture a colour widget paints itself with, and the rule that
//! decides when to build it again.

use crate::primitives::image::Image;
use crate::primitives::size::Size;
use crate::renderer::image_registry::image_handle::ImageHandle;
use crate::ui::Ui;
use glam::UVec2;
use std::num::NonZeroU32;

/// One texture a colour widget owns, filled on the CPU and rewritten in
/// place.
///
/// The colour field and the two bars are all exact per texel, which no
/// gradient and no vertex-coloured mesh can be: a gradient interpolates in
/// linear light, and mesh vertex colour is eight bits *linear*, which crushes
/// the darks. An image texture is `Rgba8UnormSrgb`, so eight bits land where
/// the eye can use them.
///
/// Kept in widget state across frames, the CPU image included. A rebuild
/// refills that image and hands it to [`ImageHandle::update`], reusing the
/// CPU buffer and GPU texture.
///
/// `K` is everything the fill reads — the field's model and hue, a bar's
/// paint — kept as itself and compared exactly, so two inputs share a
/// texture only when they are one input.
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

/// Smallest texture either axis is built at. Two texels still interpolate;
/// one would flatten the axis.
const MIN_TEXELS: u32 = 2;

/// How far below the display's resolution a surface is built, by default.
/// See [`ColorField::downsample`](crate::ColorField::downsample) for the
/// measurement behind four.
pub(crate) const DOWNSAMPLE: u32 = 4;

/// The divisor a builder was handed, once it is known to be one a surface
/// can use. One assert for the three widgets that take one.
///
/// # Panics
///
/// Panics unless `n` is a power of two from 1 to 16.
pub(crate) fn checked_downsample(n: u32) -> u32 {
    assert!(
        n.is_power_of_two() && (1..=16).contains(&n),
        "colour surface downsample must be a power of two in 1..=16, got {n}",
    );
    n
}

/// Texel dimensions for a surface covering `size` logical px on the current
/// display, reduced by `downsample` and held under the device's texture cap.
///
/// Total over every input: a size that is NaN, negative or absurd lands on
/// the floor or the cap rather than reaching the registry, and a cap below
/// the floor lowers the floor. The widgets take these numbers from
/// application layout, so they cannot assert on them.
pub(crate) fn texel_size(size: Size, downsample: u32, ui: &Ui) -> UVec2 {
    let scale = ui.display().scale_factor();
    let cap = ui.max_image_dimension().map_or(u32::MAX, NonZeroU32::get);
    let floor = MIN_TEXELS.min(cap);
    let axis = |logical: f32| {
        let texels = (logical * scale / downsample as f32).ceil();
        (texels as u32).clamp(floor, cap)
    };
    UVec2::new(axis(size.w), axis(size.h))
}

/// Nothing registered yet: what a widget's state row starts as.
impl<K> Default for ColorSurface<K> {
    fn default() -> Self {
        Self { built: None }
    }
}

impl<K: PartialEq> ColorSurface<K> {
    /// The handle to paint with, filled again first when `size` or `key`
    /// moved since the last call.
    ///
    /// `fill` writes every texel, **sRGB-encoded**:
    /// [`RgbaF32::to_srgba_u8`](crate::RgbaF32::to_srgba_u8), never the
    /// linear quantize `RgbaU8::from` performs.
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
                .register_image(&image)
                .expect("a colour surface is clamped to the device texture cap");
            self.built = Some(Built { handle, image, key });
        }
        &self.built.as_ref().unwrap().handle
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::color::srgba_u8::SrgbaU8;
    use crate::ui::harness::UiHarness;

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
