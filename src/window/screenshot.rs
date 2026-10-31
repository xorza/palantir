//! [`Screenshot`]: an image of a frame, as [`Ui::screenshot_taken`](crate::Ui::screenshot_taken)
//! hands it over, and the [`ScreenshotId`] that names the ask it answers.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::image::Image;
use glam::UVec2;

/// An image of a frame the app asked for with
/// [`Ui::request_screenshot`](crate::Ui::request_screenshot): the window or a region of it, as
/// the frame painted it.
#[derive(Clone, Debug, PartialEq)]
pub struct Screenshot {
    image: Image,
    scale: f32,
    region: Rect,
    id: ScreenshotId,
}

/// One [`Ui::request_screenshot`](crate::Ui::request_screenshot) of a window, which
/// [`Ui::screenshot_taken`](crate::Ui::screenshot_taken) reads its image by, so two asks in one
/// window cannot take each other's image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScreenshotId(u64);

/// One ask for an image: the logical region, clamped to the window, and its id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScreenshotRequest {
    pub(crate) region: Rect,
    pub(crate) id: ScreenshotId,
}

impl ScreenshotId {
    /// The first id a window hands out.
    pub(crate) const FIRST: Self = Self(1);

    /// The id after this one, which a window's next ask takes.
    pub(crate) const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl Screenshot {
    /// `image` of the logical `region`, taken at `scale`, answering the ask `id`.
    pub(crate) const fn new(image: Image, scale: f32, region: Rect, id: ScreenshotId) -> Self {
        Self {
            image,
            scale,
            region,
            id,
        }
    }

    /// The ask this image answers.
    pub(crate) const fn id(&self) -> ScreenshotId {
        self.id
    }

    /// The pixels as the frame painted them, one texel per physical pixel.
    pub const fn image(&self) -> &Image {
        &self.image
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

    /// The part of the logical `rect` that a picture `picture` texels in size shows, in the
    /// picture's texels, when the picture is this image scaled to that size, as
    /// [`Image::fitted`] scales it; `None` when it shows none of it.
    pub fn place(&self, rect: Rect, picture: UVec2) -> Option<Rect> {
        let origin = (self.region.min * self.scale).floor();
        let factor = picture.as_vec2() / self.image.size().as_vec2();
        let min = (rect.min * self.scale - origin) * factor;
        let max = (rect.max() * self.scale - origin) * factor;
        let bounds = Rect::new(0.0, 0.0, picture.x as f32, picture.y as f32);
        Rect::from_min_max(min, max).intersect(bounds)
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::paint::image::Image;
    use crate::window::screenshot::{Screenshot, ScreenshotId};
    use glam::UVec2;

    /// A 2× screenshot of the logical region at (10, 20), 20 × 10, is 40 × 20 texels from the
    /// physical origin (20, 40). Fitted to half, 20 × 10: the logical rect (14, 24, 8, 8) is
    /// physical (28, 48)–(44, 64), so (8, 8)–(24, 24) from the origin, (4, 4)–(12, 12) halved, and
    /// the picture's 10 rows cut it to (4, 4, 8, 6). A rect outside the region shows nowhere.
    #[test]
    fn a_rect_lands_in_the_fitted_picture() {
        let shot = Screenshot::new(
            Image::blank(UVec2::new(40, 20)),
            2.0,
            Rect::new(10.0, 20.0, 20.0, 10.0),
            ScreenshotId::FIRST,
        );
        let picture = UVec2::new(20, 10);
        assert_eq!(
            shot.place(Rect::new(14.0, 24.0, 8.0, 8.0), picture),
            Some(Rect::new(4.0, 4.0, 8.0, 6.0))
        );
        assert_eq!(shot.place(Rect::new(0.0, 0.0, 5.0, 5.0), picture), None);
    }
}
