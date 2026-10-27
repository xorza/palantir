//! Where a strip slot lands when the strip around it changes: the index arithmetic a tab view, a
//! dock group and a dock move share.

/// The slot a tab moved out of `from` into gap `to` lands in, with `to` addressing the strip
/// before the move (`len` appends): the gap past its own slot closes up behind it.
pub(crate) const fn landing(from: usize, to: usize) -> usize {
    if to > from { to - 1 } else { to }
}

/// Where the tab at `idx` sits after the tab at `from` moves into gap `to`.
pub(crate) const fn after_move(idx: usize, from: usize, to: usize) -> usize {
    let landing = landing(from, to);
    if idx == from {
        return landing;
    }
    let without = if idx > from { idx - 1 } else { idx };
    if without >= landing {
        without + 1
    } else {
        without
    }
}

/// Where the visible slot `idx` of a `len`-tab strip goes when the tab at `removed` leaves.
///
/// A strip is addressed by index and read by identity: a removal ahead of the visible tab slides it
/// down a slot, so a held index would switch to a tab nobody asked for. Removing the visible tab is
/// the one case that picks another: the tab that took the slot, or the one before it where the
/// slot was last. An emptied strip gives `0`.
pub(crate) const fn after_removal(idx: usize, removed: usize, len: usize) -> usize {
    debug_assert!(removed < len, "the removed slot is in the strip");
    let idx = if removed < idx { idx - 1 } else { idx };
    let last = len.saturating_sub(2);
    if idx > last { last } else { idx }
}
