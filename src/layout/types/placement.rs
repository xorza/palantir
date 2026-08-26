//! How a layer root is measured and where it lands afterwards.

use crate::layout::types::overlay::OverlayPosition;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use glam::Vec2;

/// Measurement and post-measure placement policy for one layer root.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Placement {
    Fixed { anchor: Vec2, size: Option<Size> },
    Overlay(OverlayPosition),
}

impl Placement {
    pub(crate) const fn fixed(anchor: Vec2, size: Option<Size>) -> Self {
        Self::Fixed { anchor, size }
    }

    /// Replace the anchor, keeping any size cap. An overlay position
    /// has no anchor half to keep — it resolves its origin from the
    /// measured size — so it becomes a plain fixed placement.
    pub(crate) const fn with_anchor(self, anchor: Vec2) -> Self {
        match self {
            Self::Fixed { size, .. } => Self::fixed(anchor, size),
            Self::Overlay(_) => Self::fixed(anchor, None),
        }
    }

    /// Replace the size cap, keeping the anchor, on the same terms.
    pub(crate) const fn with_size(self, size: Size) -> Self {
        match self {
            Self::Fixed { anchor, .. } => Self::fixed(anchor, Some(size)),
            Self::Overlay(_) => Self::fixed(Vec2::ZERO, Some(size)),
        }
    }

    pub(crate) fn available(self, surface: Rect) -> Size {
        match self {
            Self::Fixed { anchor, size: None } => {
                let remaining = (surface.max() - anchor).max(Vec2::ZERO);
                Size::new(remaining.x, remaining.y)
            }
            Self::Fixed {
                size: Some(size), ..
            } => Size::new(size.w.min(surface.size.w), size.h.min(surface.size.h)),
            Self::Overlay(_) => surface.size,
        }
    }

    pub(crate) fn origin(self, measured: Size, surface: Rect) -> Vec2 {
        match self {
            Self::Fixed { anchor, .. } => anchor,
            Self::Overlay(position) => position.resolve(measured, surface),
        }
    }
}

/// An anchor-relative position *is* a placement, which is what lets
/// [`LayerScope::placement`](crate::ui::layer_scope::LayerScope::placement)
/// take either form through one `impl Into<Placement>` parameter.
impl From<OverlayPosition> for Placement {
    fn from(position: OverlayPosition) -> Self {
        Self::Overlay(position)
    }
}

impl Default for Placement {
    fn default() -> Self {
        Self::fixed(Vec2::ZERO, None)
    }
}
