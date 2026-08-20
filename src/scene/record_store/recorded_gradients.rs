//! The record pass's gradient interner.

use crate::scene::record_store::recorded_gradient::RecordedGradient;
use rustc_hash::FxHashMap;

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
    /// gradient's contents. `RecordedGradient` cannot key the map itself:
    /// it is float-bearing, so it has a `PartialEq` and no `Eq`/`Hash`.
    ///
    /// One candidate per key, not a chain of them. A hit is still
    /// confirmed by equality, because being wrong there means a shape
    /// painted with someone else's gradient. What a genuine 64-bit
    /// collision costs is only the *dedup*: both gradients keep minting
    /// their own record, which is a duplicate atlas row and nothing more.
    /// Chaining the candidates would buy exact dedup in a case that does
    /// not occur, at a link array and a walk on every intern.
    ids: FxHashMap<u64, GradientId>,
}

impl RecordedGradients {
    pub(crate) fn intern(&mut self, content_hash: u64, gradient: RecordedGradient) -> GradientId {
        if let Some(&id) = self.ids.get(&content_hash)
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
        self.ids.insert(content_hash, id);
        id
    }

    pub(super) fn clear(&mut self) {
        self.records.clear();
        self.ids.clear();
    }
}
