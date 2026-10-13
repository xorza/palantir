//! [`IconSetId`] — a loaded icon set's identity. Lives in `primitives` because `shape` records it
//! through an [`IconHandle`](crate::primitives::identity::icon_handle::IconHandle).

/// Identity of a loaded icon set: a slot of the `IconRegistry` table plus the occupant's generation. Half of an [`IconHandle`](crate::IconHandle) and of the atlas cache key, hence four bytes.
///
/// The generation makes a slot reusable: without it a `Copy` handle minted before a release would silently name the next set. Sixteen bits wrap after 65 536 cycles of one slot, giving a wrong icon, not unsoundness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IconSetId {
    slot: u16,
    generation: u16,
}

impl IconSetId {
    pub(crate) const fn new(slot: u16, generation: u16) -> Self {
        Self { slot, generation }
    }

    pub(crate) const fn slot(self) -> u16 {
        self.slot
    }

    pub(crate) const fn generation(self) -> u16 {
        self.generation
    }

    pub(crate) const fn bits(self) -> u32 {
        self.slot as u32 | ((self.generation as u32) << 16)
    }
}
