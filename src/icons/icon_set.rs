//! The loaded icon set a caller's handles resolve against.

use crate::icons::icon_registry::IconSetToken;
use crate::primitives::identity::icon_handle::IconHandle;
use crate::primitives::identity::icon_id::IconId;
use crate::primitives::identity::icon_ref::IconRef;
use std::fmt;
use std::rc::Rc;

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

    /// The icon named `name`, for [`Shape::icon`](crate::widget::Shape::icon) (binary search; no
    /// allocation). Resolve once and keep the handle: it names this set, so it cannot draw
    /// another set's icon.
    pub fn by_name(&self, name: &str) -> Option<IconHandle> {
        self.inner
            .table()
            .by_name(name)
            .map(|icon| self.handle(icon))
    }

    /// The handle for `icon`, an id of this set's table.
    pub(crate) fn handle(&self, icon: IconId) -> IconHandle {
        // Resolved now so the encoder never consults the registry.
        IconHandle::new(
            IconRef {
                set: self.inner.id(),
                icon,
            },
            self.inner.table().def(icon).view_box,
        )
    }
}
