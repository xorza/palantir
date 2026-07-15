//! Sparse side-table indices for optional per-node data.

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Slot(u16);

impl Slot {
    pub(crate) const ABSENT: Self = Self(u16::MAX);

    #[inline]
    pub(crate) fn from_len(len: usize) -> Self {
        debug_assert!(
            len < Self::ABSENT.0 as usize,
            "Slot exhausted — {} entries fill the sparse-column frame; index would collide with Slot::ABSENT (got {len})",
            Self::ABSENT.0 as usize,
        );
        Self(len as u16)
    }

    #[inline]
    pub(crate) fn get(self) -> Option<usize> {
        (self.0 != Self::ABSENT.0).then_some(self.0 as usize)
    }
}

impl Default for Slot {
    #[inline]
    fn default() -> Self {
        Self::ABSENT
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ExtrasIdx {
    pub(crate) bounds: Slot,
    pub(crate) panel: Slot,
    pub(crate) chrome: Slot,
}
