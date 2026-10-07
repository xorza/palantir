//! The handles a caller names an icon with, and the loaded set they resolve
//! against.

use crate::icons::icon_registry::{IconSetId, IconSetToken};
use crate::icons::icon_table::IconId;
use crate::shape::Shape;
use crate::shape::icon::IconShape;
use glam::Vec2;
use std::fmt;
use std::rc::Rc;

/// Which icon of which loaded set. Split from [`IconHandle`] so the atlas
/// key holds identity without the size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IconRef {
    pub(crate) set: IconSetId,
    pub(crate) icon: IconId,
}

/// Names one icon of one loaded set, with the size its artwork was drawn at.
/// `Copy`, so it carries `view_box` and icon-fit resolution needs no registry
/// lookup.
///
/// **It owns nothing**, unlike [`ImageHandle`](crate::ImageHandle): the
/// [`IconSet`] keeps the set loaded, and a handle outliving every clone of it
/// panics when drawn.
///
/// From [`IconSet::handle`]; consumed by [`Shape::icon`](crate::widget::Shape::icon).
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
    /// The artwork's viewBox extent in logical px, the size to give its node.
    /// Valid to read after the set is gone; drawing is not.
    #[inline]
    pub const fn view_box(&self) -> Vec2 {
        self.view_box
    }
}

/// A loaded icon set, and an **RAII owner** of what the host caches for it:
/// set data, SVG parses and atlas rasters.
///
/// From [`Ui::load_icons`](crate::Ui::load_icons). Cloning bumps a refcount;
/// the last drop unloads everything. There is no `unload`.
///
/// **An [`IconHandle`] is not an owner**: hold the `IconSet` while anything
/// can draw from it.
#[must_use = "dropping the IconSet unloads its icons — park it in your state \
              rather than discarding load_icons' return"]
#[derive(Clone)]
pub struct IconSet {
    inner: Rc<IconSetToken>,
}

/// Manual: a derive would print every other loaded set's table through the
/// shared registry.
impl fmt::Debug for IconSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IconSet")
            .field("id", &self.inner.id())
            .field("icons", &self.inner.table().icons().len())
            .field("owners", &Rc::strong_count(&self.inner))
            .finish()
    }
}

impl IconSet {
    pub(crate) const fn from_token(inner: Rc<IconSetToken>) -> Self {
        Self { inner }
    }

    /// The handle for `icon`, for [`Shape::icon`](crate::widget::Shape::icon).
    ///
    /// # Panics
    ///
    /// If `icon` is past this set's end. An in-range id from another set is not
    /// caught.
    pub fn handle(&self, icon: IconId) -> IconHandle {
        // Resolved now so a bad id fails at the caller and the encoder never
        // consults the registry.
        IconHandle {
            icon: IconRef {
                set: self.inner.id(),
                icon,
            },
            view_box: self.inner.table().def(icon).view_box,
        }
    }

    /// Look an icon up by baked name (binary search; no allocation).
    ///
    /// Prefer the generated `IconId` constant for baked sets; this is for names
    /// that are data.
    pub fn by_name(&self, name: &str) -> Option<IconId> {
        self.inner
            .table()
            .icons()
            .binary_search_by(|def| def.name.as_ref().cmp(name))
            .ok()
            .map(|i| IconId(i as u16))
    }

    /// The icon as a shape; shorthand for `Shape::icon(set.handle(icon))`.
    ///
    /// # Panics
    ///
    /// If `icon` is past this set's end. An in-range id from another set is not
    /// caught.
    pub fn shape(&self, icon: IconId) -> IconShape {
        Shape::icon(self.handle(icon))
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::icons::icon_registry::IconSetId;
    use crate::icons::icon_set::IconRef;
    use crate::icons::icon_table::IconId;

    impl IconRef {
        /// Icon `icon` of the first generation of set slot `set`.
        pub(crate) const fn fixture(set: u16, icon: u16) -> Self {
            Self {
                set: IconSetId::new(set, 0),
                icon: IconId(icon),
            }
        }
    }
}
