use crate::icons::icon_atlas::IconId;
use crate::icons::icon_registry::{IconSetId, IconSetToken};
use crate::shape::Shape;
use crate::shape::icon::IconShape;
use glam::Vec2;
use std::rc::Rc;

/// Which icon of which loaded set — six bytes, and the whole of what the
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
/// Sixteen bytes and `Copy` — two ids plus the viewBox — so it rides through
/// record and encode like an integer.
///
/// It carries `view_box` so that resolving
/// [`IconFit`](crate::IconFit) at encode time needs no lookup: the aspect
/// ratio travels with the handle rather than being fetched from the registry
/// on the hot path.
///
/// **It owns nothing**, which is the one place it differs from
/// [`ImageHandle`](crate::ImageHandle) and the price of being `Copy`. What
/// keeps its set loaded is the [`IconSet`] the app holds; a handle that
/// outlives every clone of that set names a set the host has unloaded, and
/// panics when the renderer goes to draw it.
///
/// Handed out by [`IconSet::handle`] and consumed by
/// [`Shape::icon`](crate::Shape::icon).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconHandle {
    pub(crate) icon: IconRef,
    /// The artwork's viewBox extent in logical px.
    pub(crate) view_box: Vec2,
}

/// A loaded icon set, and an **RAII owner** of everything the host caches
/// for it: the set's data, the SVG parses the backend has paid for, and its
/// rasters in the icon atlas.
///
/// Returned by [`Ui::load_icons`](crate::Ui::load_icons). Cloning is a
/// refcount bump, so an app parks one in its state and hands copies to
/// widget constructors without ceremony; when the last clone goes, the host
/// drops all three. There is no `unload` — the `IconSet`'s lifetime *is* the
/// set's lifetime, the same bargain [`ImageHandle`](crate::ImageHandle)
/// makes.
///
/// **An [`IconHandle`] is not an owner.** It is `Copy` and holds nothing, so
/// that resolving [`IconFit`](crate::IconFit) at encode time needs no
/// lookup — which means a handle used after every `IconSet` for its set is
/// gone names a set that no longer exists, and panics when the renderer goes
/// to draw it. Hold the `IconSet` for as long as anything can draw from it.
#[must_use = "dropping the IconSet unloads its icons — park it in your state \
              rather than discarding load_icons' return"]
#[derive(Clone)]
pub struct IconSet {
    inner: Rc<IconSetToken>,
}

/// Manual rather than derived: the token holds the registry, so a derive
/// prints every *other* loaded set's icon table and SVG bytes — and the free
/// list and release queue with them — from one `dbg!` on one handle. What a
/// reader wants is which set this is and how widely it is held, which is what
/// [`ImageHandle`](crate::ImageHandle) prints for the same reason.
impl std::fmt::Debug for IconSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IconSet")
            .field("id", &self.inner.id())
            .field("icons", &self.inner.atlas().icons().len())
            .field("owners", &Rc::strong_count(&self.inner))
            .finish()
    }
}

impl IconSet {
    pub(crate) fn from_token(inner: Rc<IconSetToken>) -> Self {
        Self { inner }
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
            icon: IconRef {
                set: self.inner.id(),
                icon,
            },
            view_box: self.inner.atlas().def(icon).view_box,
        }
    }

    /// The icon's viewBox extent in logical px — the size the artwork was
    /// designed at, and what to size its node to.
    ///
    /// # Panics
    ///
    /// Panics if `icon` is not from this set.
    pub fn nominal(&self, icon: IconId) -> Vec2 {
        self.inner.atlas().def(icon).view_box
    }

    /// Look an icon up by its baked name. Binary search over the
    /// bake-sorted table — no map, no allocation.
    ///
    /// For a set baked into the binary, the generated `IconId` constant is the
    /// better path: it cannot be misspelled. This is for names that are data,
    /// such as an icon named in a config file.
    pub fn by_name(&self, name: &str) -> Option<IconId> {
        self.inner
            .atlas()
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
