//! [`ClosedPaint`]: what covers a closed shape in its record.

use crate::common::content_hash::ContentHash;
use crate::common::span::Span;
use crate::primitives::geometry::fit::Fit;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::identity::icon_handle::IconHandle;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::image::{ImageDownsample, ImageFilter, ImagePlacement};
use crate::shape::lower::LoweredPaint;
use crate::shape::paint::capture_params::CaptureParams;
use crate::shape::paint::image_source::ImageSource;

/// The paint of a closed shape's record, which the shape in [`ShapeRecord::Closed`] holds beside
/// it, as `Ui::add_shape` takes the two.
///
/// [`ShapeRecord::Closed`]: crate::shape::record::ShapeRecord::Closed
#[derive(Clone, Debug)]
pub(crate) enum ClosedPaint {
    /// A box's or a triangle's fill, border, shadow and material. A material that captures makes
    /// the shape a backdrop over the pixels painted before it.
    Fill(LoweredPaint),
    /// A registered image or a `GpuView` target, per [`ImageSource`], fit to the shape's rect and
    /// clipped to the shape. `tint` multiplies in linear-RGB premultiplied space.
    Image {
        tint: RgbaF16,
        source: ImageSource,
        placement: ImagePlacement,
        min_filter: ImageFilter,
        mag_filter: ImageFilter,
        downsample: ImageDownsample,
    },
    /// A baked SVG icon over a square-cornered box, rasterized at the exact physical size into the
    /// icon atlas. The [`IconHandle`] carries the viewBox, so `fit` needs no registry lookup.
    /// `tint` multiplies a tintable icon whole and a color icon's alpha only, see
    /// [`IconPaint`](crate::IconPaint).
    Icon {
        handle: IconHandle,
        fit: Fit,
        tint: RgbaF16,
        /// Draws a color icon as its own luminance, see
        /// [`IconPaint::desaturate`](crate::IconPaint::desaturate).
        desaturate: bool,
    },
    /// An app-written effect over the shape, which may exceed the owner. `params` indexes the
    /// `RecordStore`'s parameter words; `content_hash` covers the effect, those words, its images,
    /// its backdrop and its halo.
    Effect {
        read: RecordedRead,
        effect: EffectId,
        params: Span,
        /// Into `RecordStore::effect_images`.
        images: Span,
        content_hash: ContentHash,
    },
}

/// What an app's effect reads besides its inputs, by its kind, as its record holds it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RecordedRead {
    /// A color effect, over the shape, or a halo `outside` it alone, logical px past it.
    Color { outside: Option<f32> },
    /// A backdrop effect, over what it captures under it.
    Backdrop { capture: CaptureParams },
}

impl RecordedRead {
    /// How far past its shape the paint reaches, logical px: a halo's reach, else 0.
    pub(crate) const fn halo_reach(self) -> f32 {
        match self {
            Self::Color {
                outside: Some(reach),
            } => reach,
            Self::Color { outside: None } | Self::Backdrop { .. } => 0.0,
        }
    }
}

impl ClosedPaint {
    /// What the paint reads as a backdrop over the pixels before it: a material's or a backdrop
    /// effect's, each where it captures.
    pub(crate) const fn backdrop_capture(&self) -> Option<CaptureParams> {
        match self {
            Self::Fill(LoweredPaint {
                material: Some(material),
                ..
            }) => Some(material.capture),
            Self::Effect {
                read: RecordedRead::Backdrop { capture },
                ..
            } => Some(*capture),
            _ => None,
        }
    }
}

impl NanCheck for ClosedPaint {
    fn has_nan(&self) -> bool {
        match self {
            Self::Fill(LoweredPaint {
                fill,
                border,
                shadow,
                material,
            }) => fill.has_nan() || border.has_nan() || shadow.has_nan() || material.has_nan(),
            Self::Image {
                tint, placement, ..
            } => tint.has_nan() || placement.has_nan(),
            Self::Icon { tint, .. } => tint.has_nan(),
            Self::Effect { read, .. } => match read {
                RecordedRead::Color { outside } => outside.is_some_and(f32::is_nan),
                RecordedRead::Backdrop { capture } => capture.has_nan(),
            },
        }
    }
}
