//! A CPU-built texture a colour widget paints itself with, and the rule that
//! decides when to build it again.

use crate::primitives::image::Image;
use crate::primitives::size::Size;
use crate::renderer::image_registry::ImageHandle;
use crate::ui::Ui;
use glam::UVec2;
use rustc_hash::FxBuildHasher;
use std::hash::{BuildHasher as _, Hash};
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
/// refills that image and hands it to [`ImageHandle::update`], so a hue drag
/// allocates nothing and mints no second texture.
#[derive(Debug, Default)]
pub(crate) struct ColorSurface {
    built: Option<Built>,
    stamp: u64,
}

/// The texture and the CPU image it is refilled from.
#[derive(Debug)]
struct Built {
    handle: ImageHandle,
    image: Image,
}

/// Smallest texture either axis is built at. Two texels still interpolate;
/// one would flatten the axis.
const MIN_TEXELS: u32 = 2;

impl ColorSurface {
    /// How far below the display's resolution a surface is built, by
    /// default. See [`ColorField::downsample`](crate::ColorField::downsample)
    /// for the measurement behind four.
    pub(crate) const DOWNSAMPLE: u32 = 4;

    /// The divisor a builder was handed, once it is known to be one the
    /// surface can use. One assert for the three widgets that take one.
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

    /// Texel dimensions for a surface covering `size` logical px on the
    /// current display, reduced by `downsample` and held under the device's
    /// texture cap.
    ///
    /// Total over every input: a size that is NaN, negative or absurd lands
    /// on the floor or the cap rather than reaching the registry. The widgets
    /// take these numbers from application layout, so they cannot assert on
    /// them.
    pub(crate) fn texel_size(size: Size, downsample: u32, ui: &Ui) -> UVec2 {
        let scale = ui.display().scale_factor();
        let cap = ui.max_image_dimension().map_or(u32::MAX, NonZeroU32::get);
        let axis = |logical: f32| {
            let texels = (logical * scale / downsample as f32).ceil();
            (texels as u32).clamp(MIN_TEXELS, cap.max(MIN_TEXELS))
        };
        UVec2::new(axis(size.w), axis(size.h))
    }

    /// The handle to paint with, filled again first when `size` or `key`
    /// moved since the last call.
    ///
    /// `key` is everything `fill` reads, and each surface brings a different
    /// set — the field's hue and model, a bar's paint — which is why it is
    /// hashed to one number here rather than kept as a struct carrying fields
    /// one of them ignores.
    ///
    /// `fill` writes every texel, **sRGB-encoded**:
    /// [`RgbaF32::to_srgba_u8`](crate::RgbaF32::to_srgba_u8), never the
    /// linear quantize `RgbaU8::from` performs.
    pub(crate) fn ensure(
        &mut self,
        ui: &Ui,
        size: UVec2,
        key: impl Hash,
        fill: impl FnOnce(&mut Image),
    ) -> &ImageHandle {
        let stamp = FxBuildHasher.hash_one(key);
        let built = match self.built.take() {
            Some(mut built) if built.handle.size() == size => {
                if stamp != self.stamp {
                    fill(&mut built.image);
                    built.handle.update(&built.image);
                }
                built
            }
            // First build or a resize. A resize is rare — a panel being
            // dragged wider — so the allocation is not on any hot path.
            _ => {
                let mut image = Image::blank(size);
                fill(&mut image);
                let handle = ui
                    .register_image(&image)
                    .expect("a colour surface is clamped to the device texture cap");
                Built { handle, image }
            }
        };
        self.stamp = stamp;
        &self.built.insert(built).handle
    }
}
