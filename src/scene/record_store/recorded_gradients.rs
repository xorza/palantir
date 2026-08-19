//! The record pass's gradient interner.

use crate::scene::record_store::recorded_gradient::RecordedGradient;
use rustc_hash::FxHashMap;

const GRADIENT_CHAIN_END: u32 = u32::MAX;

/// Record-local handle into [`RecordedGradients::records`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GradientId(pub(crate) u32);

/// Record-local gradient content and interning metadata under one reset boundary.
#[derive(Default, Debug)]
pub(crate) struct RecordedGradients {
    pub(crate) records: Vec<RecordedGradient>,
    heads: FxHashMap<u64, GradientId>,
    next: Vec<u32>,
}

impl RecordedGradients {
    pub(crate) fn intern(&mut self, content_hash: u64, gradient: RecordedGradient) -> GradientId {
        let head = self
            .heads
            .get(&content_hash)
            .copied()
            .map_or(GRADIENT_CHAIN_END, |id| id.0);
        let mut current = head;
        while current != GRADIENT_CHAIN_END {
            let idx = current as usize;
            // Equality confirmation keeps true hash collisions correct.
            if self.records[idx] == gradient {
                return GradientId(current);
            }
            current = self.next[idx];
        }

        debug_assert!(
            self.records.len() < GRADIENT_CHAIN_END as usize,
            "recorded gradient count exceeds the u32 handle range",
        );
        let id = GradientId(self.records.len() as u32);
        self.records.push(gradient);
        self.next.push(head);
        self.heads.insert(content_hash, id);
        id
    }

    pub(super) fn clear(&mut self) {
        self.records.clear();
        self.heads.clear();
        self.next.clear();
    }
}
