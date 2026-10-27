//! [`IconHandle`] — one icon of one loaded set. Lives in `primitives` because `shape` records it.

use crate::primitives::identity::icon_ref::IconRef;
use glam::Vec2;

/// Names one icon of one loaded set, with the size its artwork was drawn at.
/// `Copy`, so it carries `view_box` and icon-fit resolution needs no registry
/// lookup.
///
/// **It owns nothing**, unlike [`ImageHandle`](crate::ImageHandle): the
/// [`IconSet`](crate::IconSet) keeps the set loaded, and a handle outliving every clone of it
/// panics when drawn.
///
/// From [`IconSet::by_name`](crate::IconSet::by_name); consumed by [`Shape::icon`](crate::widget::Shape::icon).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconHandle {
    pub(crate) icon: IconRef,
    /// The artwork's viewBox extent in logical px.
    ///
    /// Read-only: the shape hash omits it because one `(set, icon)` pair has one
    /// value, so a writable field could move the painted rect unseen.
    view_box: Vec2,
}

impl IconHandle {
    pub(crate) const fn new(icon: IconRef, view_box: Vec2) -> Self {
        Self { icon, view_box }
    }

    /// The artwork's viewBox extent in logical px, the size to give its node.
    /// Valid to read after the set is gone; drawing is not.
    #[inline]
    pub const fn view_box(&self) -> Vec2 {
        self.view_box
    }
}
