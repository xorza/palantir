use crate::icons::icon_atlas::{IconAtlas, IconId};
use crate::icons::icon_registry::IconSetId;
use crate::shape::Shape;
use crate::shape::icon::IconShape;
use glam::Vec2;
use std::rc::Rc;

/// Which icon of which loaded set — four bytes, and the whole of what the
/// atlas caches against. Split out of [`IconHandle`] so
/// [`IconRasterKey`](crate::icons::icon_raster_key::IconRasterKey) can hold an
/// icon's identity without also holding its size, which the key expresses in
/// physical pixels instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IconRef {
    pub(crate) set: IconSetId,
    pub(crate) icon: IconId,
}

/// Names one icon of one loaded set, with the size its artwork was drawn at.
/// Twelve bytes and `Copy`: the baked data is `'static`, so there is nothing
/// to reference-count and no lifetime to hold — pass it around like an
/// integer.
///
/// It carries `view_box` so that resolving
/// [`IconFit`](crate::IconFit) at encode time needs no lookup: the aspect
/// ratio travels with the handle rather than being fetched from the registry
/// on the hot path.
///
/// Handed out by [`IconSet::handle`] and consumed by
/// [`Shape::icon`](crate::Shape::icon).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconHandle {
    pub(crate) icon: IconRef,
    /// The artwork's viewBox extent in logical px.
    pub(crate) view_box: Vec2,
}

/// A loaded icon set: the baked data plus the id the renderer resolves it by.
///
/// Returned by [`Ui::load_icons`](crate::Ui::load_icons). Cloning is a
/// refcount bump, so an app parks one in its state and hands copies to widget
/// constructors without ceremony — and the set is freed when the last one
/// goes, rather than living as long as the process.
#[derive(Clone, Debug)]
pub struct IconSet {
    id: IconSetId,
    atlas: Rc<IconAtlas>,
}

impl IconSet {
    pub(crate) fn new(id: IconSetId, atlas: Rc<IconAtlas>) -> Self {
        Self { id, atlas }
    }

    /// The handle for `icon`, to hand to [`Shape::icon`](crate::Shape::icon).
    ///
    /// # Panics
    ///
    /// Panics if `icon` is not from this set.
    pub fn handle(&self, icon: IconId) -> IconHandle {
        // Resolved now rather than at draw time, so an id that crossed
        // between sets fails at the call site that mixed them — and so the
        // encoder never has to consult the registry for an aspect ratio.
        IconHandle {
            icon: IconRef { set: self.id, icon },
            view_box: self.atlas.def(icon).view_box,
        }
    }

    /// The icon's viewBox extent in logical px — the size the artwork was
    /// designed at, and what to size its node to.
    ///
    /// # Panics
    ///
    /// Panics if `icon` is not from this set.
    pub fn nominal(&self, icon: IconId) -> Vec2 {
        self.atlas.def(icon).view_box
    }

    /// Look an icon up by its baked name. Binary search over the
    /// bake-sorted table — no map, no allocation.
    ///
    /// For a set baked into the binary, the generated `IconId` constant is the
    /// better path: it cannot be misspelled. This is for names that are data,
    /// such as an icon named in a config file.
    pub fn by_name(&self, name: &str) -> Option<IconId> {
        self.atlas
            .icons()
            .binary_search_by_key(&name, |def| def.name)
            .ok()
            .map(|i| IconId(i as u16))
    }

    /// The icon as a shape, ready to add — the same as
    /// `Shape::icon(set.handle(icon))`, which is the call this exists to
    /// shorten because it is the one every draw site makes.
    ///
    /// # Panics
    ///
    /// Panics if `icon` is not from this set.
    pub fn shape(&self, icon: IconId) -> IconShape {
        Shape::icon(self.handle(icon))
    }
}
