//! [`IconRef`] — which icon of which loaded set. Lives in `primitives` because `shape` records it
//! through an [`IconHandle`](crate::primitives::identity::icon_handle::IconHandle).

use crate::primitives::identity::icon_id::IconId;
use crate::primitives::identity::icon_set_id::IconSetId;

/// Which icon of which loaded set. Split from [`IconHandle`](crate::primitives::identity::icon_handle::IconHandle) so the atlas
/// key holds identity without the size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IconRef {
    pub(crate) set: IconSetId,
    pub(crate) icon: IconId,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::identity::icon_id::IconId;
    use crate::primitives::identity::icon_ref::IconRef;
    use crate::primitives::identity::icon_set_id::IconSetId;

    impl IconRef {
        /// Icon `icon` of the first generation of set slot `set`.
        pub(crate) const fn fixture(set: u16, icon: u16) -> Self {
            Self {
                set: IconSetId::new(set, 0),
                icon: IconId::new(icon),
            }
        }
    }
}
