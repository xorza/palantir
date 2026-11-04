//! [`MeasureColumns`]: what a measure leaves on each node, as the scratch, the snapshot arena and
//! a cache hit hold it.

use crate::layout::cache::UNMEASURED;
use crate::layout::intrinsic::len_req::SLOT_COUNT;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::offer;

/// One row per node of what a measure found: each column declared once, here, and held whole by
/// the engine's scratch for one tree and by the snapshot arena for every captured tree in a row.
/// A capture borrows the scratch's and a cache hit borrows the arena's with the range its subtree
/// takes. Refused: the same columns declared again on the scratch, the arena, the capture input
/// and the cache hit, where a new column had to be added four times.
#[derive(Debug, Default)]
pub(super) struct MeasureColumns {
    pub(super) desired: Vec<Size>,
    /// Each node's measured floor, margin-inclusive; see
    /// [`Measured`](crate::layout::measured::Measured). Arrange places a node at it when its slot
    /// is smaller than `desired`.
    pub(super) floor: Vec<Size>,
    /// Each node's [`Measured::stable_from`](crate::layout::measured::Measured::stable_from).
    /// Read by no pass after measure; kept for the next capture.
    pub(super) stable_from: Vec<Size>,
    pub(super) intrinsics: Vec<[f32; SLOT_COUNT]>,
    /// The offer each node was measured at; [`UNMEASURED`] where no measure reached it.
    pub(super) available: Vec<Size>,
}

impl MeasureColumns {
    /// `n` rows of a tree no measure reached yet. Destructured so a new column cannot be left
    /// un-reset.
    pub(super) fn resize_for(&mut self, n: usize) {
        let Self {
            desired,
            floor,
            stable_from,
            intrinsics,
            available,
        } = self;
        desired.clear();
        desired.resize(n, Size::ZERO);
        floor.clear();
        floor.resize(n, Size::ZERO);
        // A node measure never reaches, below a collapsed one, claims no range.
        stable_from.clear();
        stable_from.resize(n, Size::new(offer::AT_OFFER_ONLY, offer::AT_OFFER_ONLY));
        intrinsics.clear();
        intrinsics.resize(n, [f32::NAN; SLOT_COUNT]);
        available.clear();
        available.resize(n, UNMEASURED);
    }

    /// No rows, capacity kept.
    pub(super) fn clear(&mut self) {
        let Self {
            desired,
            floor,
            stable_from,
            intrinsics,
            available,
        } = self;
        desired.clear();
        floor.clear();
        stable_from.clear();
        intrinsics.clear();
        available.clear();
    }

    /// Append every row of `rows`.
    pub(super) fn extend_from(&mut self, rows: &Self) {
        let Self {
            desired,
            floor,
            stable_from,
            intrinsics,
            available,
        } = self;
        desired.extend_from_slice(&rows.desired);
        floor.extend_from_slice(&rows.floor);
        stable_from.extend_from_slice(&rows.stable_from);
        intrinsics.extend_from_slice(&rows.intrinsics);
        available.extend_from_slice(&rows.available);
    }

    /// The row count, which every column holds.
    pub(super) fn len(&self) -> usize {
        let Self {
            desired,
            floor,
            stable_from,
            intrinsics,
            available,
        } = self;
        debug_assert_eq!(floor.len(), desired.len());
        debug_assert_eq!(stable_from.len(), desired.len());
        debug_assert_eq!(intrinsics.len(), desired.len());
        debug_assert_eq!(available.len(), desired.len());
        desired.len()
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::layout::measure_columns::MeasureColumns;

    impl MeasureColumns {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                desired,
                floor,
                stable_from,
                intrinsics,
                available,
            } = self;
            scratch_report::vec_bytes(desired)
                + scratch_report::vec_bytes(floor)
                + scratch_report::vec_bytes(stable_from)
                + scratch_report::vec_bytes(intrinsics)
                + scratch_report::vec_bytes(available)
        }
    }
}
