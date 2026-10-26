//! [`Screenshot`]: an image of a frame, as [`Ui::screenshot_taken`](crate::Ui::screenshot_taken)
//! hands it over.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::image::Image;

/// An image of a frame the app asked for with
/// [`Ui::request_screenshot`](crate::Ui::request_screenshot): the window or a region of it, as
/// the frame painted it.
#[derive(Clone, Debug, PartialEq)]
pub struct Screenshot {
    image: Option<Image>,
    scale: f32,
    region: Rect,
}

impl Screenshot {
    /// `image` of the logical `region`, taken at `scale`; `None` for an empty region.
    pub(crate) const fn new(image: Option<Image>, scale: f32, region: Rect) -> Self {
        Self {
            image,
            scale,
            region,
        }
    }

    /// The pixels as the frame painted them, one texel per physical pixel; `None` when the region
    /// was empty and painted nothing.
    pub const fn image(&self) -> Option<&Image> {
        self.image.as_ref()
    }

    /// Physical pixels per logical pixel when the frame was painted.
    pub const fn scale(&self) -> f32 {
        self.scale
    }

    /// The logical rect the image covers, after the request was clamped to the window. Its
    /// edges, scaled, round out to the whole physical pixels the image holds.
    pub const fn region(&self) -> Rect {
        self.region
    }
}
