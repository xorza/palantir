//! [`Screenshot`]: an image of a frame, as [`Ui::screenshot_taken`](crate::Ui::screenshot_taken)
//! hands it over, and the [`ScreenshotId`] that names the ask it answers.

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
    /// `image` of the logical `region`, taken at `scale`, answering the ask `id`; `None` for an
    /// empty region.
    pub(crate) const fn new(
        image: Option<Image>,
        scale: f32,
        region: Rect,
        id: ScreenshotId,
    ) -> Self {
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
