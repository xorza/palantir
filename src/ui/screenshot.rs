//! [`Screenshot`]: an image of a frame, as [`Ui::screenshot_taken`](crate::Ui::screenshot_taken)
//! hands it over.

use crate::primitives::geometry::rect::Rect;
use glam::UVec2;

/// An image of a frame the app asked for with
/// [`Ui::request_screenshot`](crate::Ui::request_screenshot): the window or a region of it, as
/// the frame painted it.
#[derive(Clone, Debug, PartialEq)]
pub struct Screenshot {
    pixels: Vec<u8>,
    size: UVec2,
    scale: f32,
    region: Rect,
}

impl Screenshot {
    /// `pixels` of `size` physical pixels, taken at `scale` over the logical `region`.
    pub(crate) fn new(pixels: Vec<u8>, size: UVec2, scale: f32, region: Rect) -> Self {
        debug_assert_eq!(
            pixels.len(),
            size.x as usize * size.y as usize * 4,
            "one RGBA8 pixel per physical pixel"
        );
        Self {
            pixels,
            size,
            scale,
            region,
        }
    }

    /// The pixels, row by row from the top, four bytes each: red, green, blue, alpha, encoded
    /// sRGB as an image file holds them.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Width and height in physical pixels.
    pub const fn size(&self) -> UVec2 {
        self.size
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
