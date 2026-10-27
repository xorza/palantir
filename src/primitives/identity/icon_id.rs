//! [`IconId`] — an icon's index within its set. Lives in `primitives` because `shape` records it
//! through an [`IconHandle`](crate::primitives::identity::icon_handle::IconHandle).

/// Index of one icon within its `IconTable`. Crate-private: a caller holds an
/// [`IconHandle`](crate::IconHandle), which names the set too, so an index cannot
/// cross sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IconId(u16);

impl IconId {
    /// Icon `idx` of a set: minted by its table, and by tests that name one without a table.
    pub(crate) const fn new(idx: u16) -> Self {
        Self(idx)
    }

    pub(crate) const fn idx(self) -> u16 {
        self.0
    }
}
