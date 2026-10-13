//! [`IconId`] — an icon's index within its set. Lives in `primitives` because `shape` records it
//! through an [`IconHandle`](crate::primitives::identity::icon_handle::IconHandle).

/// Index of one icon within its `IconTable`, resolved through
/// [`IconSet::by_name`](crate::IconSet::by_name). The table mints every id, so
/// an id always names an icon of the set it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IconId(u16);

impl IconId {
    /// Icon `index` of a set: minted by its table, and by tests that name one without a table.
    pub(crate) const fn new(index: u16) -> Self {
        Self(index)
    }

    pub(crate) const fn index(self) -> u16 {
        self.0
    }
}
