//! A CPU-built texture a colour widget paints itself with, and the rule that
//! decides when to build it again.

use crate::common::hash::Hasher;
use crate::primitives::image::Image;
use crate::primitives::size::Size;
use crate::renderer::image_registry::ImageHandle;
use crate::ui::Ui;
use glam::UVec2;
use std::hash::{Hash, Hasher as _};
use std::num::NonZeroU32;

/// One texture a colour widget owns, filled on the CPU and refreshed in place.
///
/// The colour field and the two bars are all exact per texel, which no
/// gradient and no vertex-coloured mesh can be: a gradient interpolates in
/// linear light, and mesh vertex colour is eight bits *linear*, which crushes
/// the darks. An image texture is `Rgba8UnormSrgb`, so eight bits land where
/// the eye can use them.
///
/// Kept in widget state across frames. A rebuild refills one retained buffer
/// and calls [`ImageHandle::update`], so a hue drag allocates nothing and
/// mints no second texture.
#[derive(Debug, Default)]
pub(crate) struct ColorSurface {
    handle: Option<ImageHandle>,
    texels: Vec<u8>,
    size: UVec2,
    stamp: u64,
}

/// Bytes per texel. `Rgba8UnormSrgb`, and the alpha is written too.
const CHANNELS: usize = 4;

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

    /// The handle to paint with, rebuilding the texels first when `size` or
    /// `stamp` moved since the last call.
    ///
    /// `stamp` is the caller's own hash of everything its `fill` reads. Each
    /// surface hashes a different set — the field's hue and model, the bar's
    /// model, the alpha bar's colour — which is why the key is one number
    /// here rather than a struct carrying fields two of the three ignore.
    ///
    /// `fill` writes exactly `size.x * size.y * 4` bytes into the cleared
    /// buffer, **sRGB-encoded**: [`RgbaF32::to_srgba_u8`](crate::RgbaF32::to_srgba_u8),
    /// never the linear quantize `RgbaU8::from` performs.
    pub(crate) fn ensure(
        &mut self,
        ui: &Ui,
        size: UVec2,
        stamp: u64,
        fill: impl FnOnce(&mut Vec<u8>, UVec2),
    ) -> &ImageHandle {
        if self.handle.is_none() || size != self.size || stamp != self.stamp {
            self.rebuild(ui, size, stamp, fill);
        }
        self.handle
            .as_ref()
            .expect("a rebuild always leaves a handle behind")
    }

    /// Fold whatever a fill reads into the one number [`Self::ensure`]
    /// compares.
    pub(crate) fn stamp(parts: impl Hash) -> u64 {
        let mut hasher = Hasher::new();
        parts.hash(&mut hasher);
        hasher.finish()
    }

    fn rebuild(
        &mut self,
        ui: &Ui,
        size: UVec2,
        stamp: u64,
        fill: impl FnOnce(&mut Vec<u8>, UVec2),
    ) {
        let needed = size.x as usize * size.y as usize * CHANNELS;
        self.texels.clear();
        self.texels.reserve_exact(needed);
        fill(&mut self.texels, size);
        debug_assert_eq!(
            self.texels.len(),
            needed,
            "a colour surface fill owes exactly one RGBA texel per pixel",
        );
        match &self.handle {
            Some(handle) if size == self.size => handle.update(&self.texels),
            // First build or a resize. A resize is rare — a panel being
            // dragged wider — so the clone it costs is not on any hot path.
            _ => {
                let image = Image::from_rgba8(size.x, size.y, self.texels.clone());
                let handle = ui
                    .register_image(image)
                    .expect("a colour surface is clamped to the device texture cap");
                self.handle = Some(handle);
                self.size = size;
            }
        }
        self.stamp = stamp;
    }
}
