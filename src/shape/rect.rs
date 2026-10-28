//! The rectangle builder. Lowers to `ShapeRecord::Quad(QuadShape::Rect)`.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::lower;
use crate::shape::paint::lowered_backdrop::LoweredBackdrop;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::sealed;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RectKind {
    /// Fill inside the rounded boundary.
    Rounded = 0,
    /// Fill outside the rounded boundary, leaving its interior transparent.
    Windowed = 1,
}

/// Filled and/or bordered rectangle.
#[derive(Clone, Debug)]
#[must_use]
pub struct RectShape {
    pub(crate) kind: RectKind,
    pub(crate) local_rect: Option<Rect>,
    pub(crate) corners: Corners,
    pub(crate) fill: Brush,
    pub(crate) border: Stroke,
    pub(crate) backdrop_filter: BackdropFilter,
}

impl RectShape {
    pub(super) const fn new(kind: RectKind, local_rect: Option<Rect>) -> Self {
        Self {
            kind,
            local_rect,
            corners: Corners::ZERO,
            fill: Brush::TRANSPARENT,
            border: Stroke::NONE,
            backdrop_filter: BackdropFilter::NONE,
        }
    }

    /// Interior paint.
    ///
    /// # Panics
    ///
    /// Panics unless a solid fill is a [color](crate::widget::domain::color) and a gradient's geometry holds the kinds [`Chrome`](crate::Chrome) lists.
    #[track_caller]
    pub fn fill(mut self, fill: impl Into<Brush>) -> Self {
        let fill = fill.into();
        fill.validate();
        self.fill = fill;
        self
    }

    /// Edge paint inside the boundary; the border's outer edge is the rect's edge.
    ///
    /// # Panics
    ///
    /// Panics unless the width is a [length](crate::widget::domain::length) and
    /// the color a [color](crate::widget::domain::color).
    #[track_caller]
    pub const fn border(mut self, border: Stroke) -> Self {
        border.validate();
        self.border = border;
        self
    }

    /// Corner radii. Takes one number for all four, or a [`Corners`].
    ///
    /// # Panics
    ///
    /// Panics unless every radius is a [length](crate::widget::domain::length) and a
    /// [lane](crate::widget::domain::lane).
    #[track_caller]
    pub fn corners(mut self, corners: impl Into<Corners>) -> Self {
        let corners = corners.into();
        corners.validate();
        self.corners = corners;
        self
    }

    /// Show the pixels painted before the rect through `filter`, blurred and shaded, under its
    /// fill, which tints them: a pane. A gradient fill and the border paint over the pane.
    ///
    /// # Panics
    ///
    /// Panics unless `filter.blur` is a [length](crate::widget::domain::length), its
    /// `saturation` and `noise` [factors](crate::widget::domain::factor), and its lens's
    /// `bezel` and `thickness` lengths and `refractive_index` at least 1, each a
    /// [lane](crate::widget::domain::lane).
    #[track_caller]
    pub const fn backdrop_filter(mut self, filter: BackdropFilter) -> Self {
        filter.validate();
        self.backdrop_filter = filter;
        self
    }
}

impl sealed::LowerShape for RectShape {
    fn is_noop(&self) -> bool {
        self.local_rect.is_some_and(Rect::is_paint_empty)
            || (self.fill.is_noop() && self.border.is_noop() && self.backdrop_filter.is_noop())
    }

    /// Geometry is already in storage form; the fill interns through [`lower::brush`], the pool chrome shares.
    fn lower(self, store: &mut RecordStore) -> ShapeRecord {
        let Self {
            kind,
            local_rect,
            corners,
            fill,
            border,
            backdrop_filter,
        } = self;
        ShapeRecord::Quad(QuadShape::Rect {
            kind,
            local_rect,
            corners,
            fill: lower::brush(store, &fill),
            border: ShapeStroke::from(border),
            backdrop: LoweredBackdrop::from(backdrop_filter),
        })
    }
}
