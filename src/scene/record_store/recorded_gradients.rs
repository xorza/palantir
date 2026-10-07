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
    /// `content_hash → the record last minted under it`; the caller's hash is reused, as `RecordedGradient` is float-bearing and has no `Eq`/`Hash`.
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

/// Slots per indexed record, so the table sits at half load.
const SLOTS_PER_RECORD: usize = 2;

/// Smallest table, so the first gradient does not mint a two-slot one.
const MIN_SLOTS: usize = 64;

/// `content_hash → the record last minted under it`, direct-mapped and stamped with the frame that wrote each slot.
///
/// **One candidate per slot.** A hit is confirmed by equality; a collision only costs dedup (a duplicate atlas row), which a probe chain would not repay.
///
/// **Stamped rather than cleared**: a table sized by the session's peak would make every frame pay to clear it; a stamp makes the reset one integer.
#[derive(Debug)]
struct GradientIndex {
    slots: Vec<GradientSlot>,
    /// Serial of the current frame, never zero, so zeroed slots read as absent.
    stamp: u32,
}

/// One direct-mapped slot: the record it names and the frame that wrote it.
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

    /// The hash is a full 64-bit content hash, so its low bits need no mixing.
    #[inline]
    fn at(&self, content_hash: u64) -> usize {
        debug_assert!(
            !self.slots.is_empty(),
            "the gradient index is read only after `widen_for` sized it",
        );
        content_hash as usize & (self.slots.len() - 1)
    }

    /// Widen to index `records` records at [`SLOTS_PER_RECORD`], unless the table already does. The widened table starts empty (a record keeps no hash), costing only dedup for the rest of the frame.
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
            // Four billion frames on, a stale slot would read as ours and hand back an id past `records`; one walk makes the wrap unreachable.
            self.slots.fill(GradientSlot::default());
            self.stamp = 1;
        }
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::scene::record_store::recorded_gradients::RecordedGradients;

    impl RecordedGradients {
        /// Wind the frame serial to its last value so one `clear` steps over the wrap.
        pub(crate) fn wind_index_to_last_frame(&mut self) {
            self.index.stamp = u32::MAX;
        }
    }
}
