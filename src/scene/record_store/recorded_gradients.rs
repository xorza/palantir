//! The record pass's gradient interner.

use crate::scene::record_store::recorded_gradient::RecordedGradient;

/// Record-local handle into [`RecordedGradients::records`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GradientId(pub(crate) u32);

/// Record-local gradient content and interning metadata under one reset boundary.
#[derive(Default, Debug)]
pub(crate) struct RecordedGradients {
    pub(crate) records: Vec<RecordedGradient>,
    /// `content_hash → the record last minted under it`. The hash comes
    /// from the caller, which computed it anyway to stamp on the shape
    /// record — so interning costs a probe, not a second hash of the
    /// gradient's contents. `RecordedGradient` cannot key an index
    /// itself: it is float-bearing, so it has a `PartialEq` and no
    /// `Eq`/`Hash`.
    index: GradientIndex,
}

impl RecordedGradients {
    pub(crate) fn intern(&mut self, content_hash: u64, gradient: RecordedGradient) -> GradientId {
        self.index.widen_for(self.records.len() + 1);
        if let Some(id) = self.index.get(content_hash)
            && self.records[id.0 as usize] == gradient
        {
            return id;
        }
        debug_assert!(
            self.records.len() < u32::MAX as usize,
            "recorded gradient count exceeds the u32 handle range",
        );
        let id = GradientId(self.records.len() as u32);
        self.records.push(gradient);
        self.index.put(content_hash, id);
        id
    }

    pub(super) fn clear(&mut self) {
        self.records.clear();
        self.index.reset();
    }
}

/// Slots the table holds per record it indexes, so a frame's gradients
/// sit at half load and collisions stay rare.
const SLOTS_PER_RECORD: usize = 2;

/// Smallest table, so the first gradient of a session does not mint a
/// two-slot one and then widen it on every gradient after.
const MIN_SLOTS: usize = 64;

/// `content_hash → the record last minted under it`, direct-mapped and
/// stamped with the frame that wrote each slot.
///
/// **One candidate per slot, not a chain of them.** A hit is still
/// confirmed by equality, because being wrong there means a shape painted
/// with someone else's gradient. What a collision costs is only the
/// *dedup*: both gradients keep minting their own record, which is a
/// duplicate atlas row and nothing more. That is what makes direct
/// mapping the right shape — a probe chain would buy exact dedup in a
/// case that does not occur, at a walk on every intern.
///
/// **Stamped rather than cleared**, because this index is built from
/// nothing every frame. A hash map's `clear` walks the whole table, and
/// the table is sized by the session's peak gradient count — so every
/// frame pays for the busiest one to discover the index is empty. A stamp
/// makes the reset one integer.
#[derive(Debug)]
struct GradientIndex {
    slots: Vec<GradientSlot>,
    /// Serial of the current frame, never zero — so a zeroed slot reads
    /// as absent and a fresh or widened table needs no writing.
    stamp: u32,
}

/// One direct-mapped slot: the record it names and the frame that wrote
/// it.
#[derive(Clone, Copy, Debug, Default)]
struct GradientSlot {
    stamp: u32,
    id: u32,
}

impl Default for GradientIndex {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            stamp: 1,
        }
    }
}

impl GradientIndex {
    /// The record `content_hash` last minted *this frame*, if any.
    #[inline]
    fn get(&self, content_hash: u64) -> Option<GradientId> {
        let slot = self.slots[self.at(content_hash)];
        (slot.stamp == self.stamp).then_some(GradientId(slot.id))
    }

    #[inline]
    fn put(&mut self, content_hash: u64, id: GradientId) {
        let at = self.at(content_hash);
        self.slots[at] = GradientSlot {
            stamp: self.stamp,
            id: id.0,
        };
    }

    /// The caller's hash is a full 64-bit content hash, so the low bits
    /// are as good a slot as any and need no mixing.
    #[inline]
    fn at(&self, content_hash: u64) -> usize {
        debug_assert!(
            !self.slots.is_empty(),
            "the gradient index is read only after `widen_for` sized it",
        );
        content_hash as usize & (self.slots.len() - 1)
    }

    /// Widen to index `records` records at [`SLOTS_PER_RECORD`], unless
    /// the table already does.
    ///
    /// The widened table starts empty. Nothing here can carry the old
    /// hints across, because a record does not keep the hash it was
    /// minted under — so what a widening costs is the dedup for the rest
    /// of the frame that triggered it, which is the same duplicate record
    /// a collision costs and which this index already treats as the
    /// acceptable outcome. The next frame indexes at the new width from
    /// its first gradient.
    fn widen_for(&mut self, records: usize) {
        let want = (records * SLOTS_PER_RECORD)
            .max(MIN_SLOTS)
            .next_power_of_two();
        if self.slots.len() >= want {
            return;
        }
        self.slots.clear();
        self.slots.resize(want, GradientSlot::default());
    }

    /// Start a new frame.
    fn reset(&mut self) {
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            // Four billion frames on, a slot left by the frame that last
            // held this serial would read as ours and hand back an id
            // past the end of `records`. One walk makes the wrap
            // unreachable instead of merely unlikely.
            self.slots.fill(GradientSlot::default());
            self.stamp = 1;
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::scene::record_store::recorded_gradients::RecordedGradients;

    impl RecordedGradients {
        /// Wind the index's frame serial to its last value, so one
        /// `clear` steps it over the wrap.
        pub(crate) fn wind_index_to_last_frame(&mut self) {
            self.index.stamp = u32::MAX;
        }
    }
}
