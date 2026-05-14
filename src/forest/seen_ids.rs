//! Per-frame `WidgetId` tracker. Owns three things that all key off
//! "which widgets were recorded this frame":
//!
//! 1. **Collision detection + disambiguation.** [`Self::record`]
//!    rewrites `element.id` when two widgets land on the same id this
//!    frame by mixing in an occurrence counter — duplicate raw ids
//!    would silently corrupt every per-id store (focus, scroll, click
//!    capture, hit-test), so we always disambiguate. Explicit-key
//!    collisions are caller bugs: [`Self::record`] returns
//!    [`RecordOutcome::DisambiguatedExplicit`] carrying the
//!    first-occurrence node's `NodeId` so [`crate::forest::Forest`]
//!    can pair both colliding nodes for the always-on magenta debug
//!    overlay emitted by the encoder.
//! 2. **Removed-widget diff + rollover.** [`Self::rollover`] computes
//!    which ids were present last painted frame but absent this pass
//!    (populating `removed` for [`crate::ui::damage::DamageEngine`] /
//!    [`crate::text::TextShaper`] / measure cache / state /
//!    animation), then swaps `curr → prev` so the next frame diffs
//!    against this one. Called once per `run_frame` from
//!    [`crate::Ui::finalize_frame`]; discarded record passes don't
//!    touch seen-id state, so `prev` stays anchored at the last
//!    *painted* frame regardless of how many discard passes ran.

use crate::forest::element::Element;
use crate::forest::tree::{Layer, NodeId};
use crate::primitives::widget_id::WidgetId;
use rustc_hash::{FxHashMap, FxHashSet};

/// How a `WidgetId` was produced. Both sources share the same
/// occurrence-counter disambiguation when they collide; the variant
/// is preserved so [`SeenIds::record`] can flag explicit collisions
/// (caller bug) for the always-on magenta debug overlay while
/// leaving auto collisions silent (expected from loops / helpers).
#[derive(Clone, Copy, Debug)]
pub(crate) enum IdSource {
    /// Caller passed an explicit key via `.id_salt(...)`. Collisions
    /// are caller bugs — the disambiguated node gets a magenta outline.
    Explicit,
    /// Id minted by `WidgetId::auto_stable()` (track-caller). Collisions
    /// are expected in loops / helper closures and get disambiguated
    /// silently.
    Auto,
}

/// Result of [`SeenIds::record`]. The `Forest` reads this to decide
/// whether to pair both colliding nodes for the debug overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecordOutcome {
    /// First time this id was seen this frame — no rewrite.
    Inserted,
    /// Auto-source collision — silently disambiguated.
    DisambiguatedAuto,
    /// Explicit-source collision — disambiguated; carries the
    /// first-occurrence node's `(Layer, NodeId)` so the `Forest` can
    /// pair both colliding nodes for the overlay without a tree
    /// scan, even when the two endpoints are in different layers.
    DisambiguatedExplicit { first: (Layer, NodeId) },
}

#[derive(Default)]
pub(crate) struct SeenIds {
    /// `WidgetId → (Layer, NodeId)` of every widget recorded this
    /// frame so far. Populated by [`Self::record`] during `Ui::node`;
    /// the value enables O(1) first-node lookup on explicit
    /// collisions (avoids a tree scan in the encoder) and preserves
    /// the originating layer so cross-layer collisions resolve their
    /// arranged rects correctly.
    curr: FxHashMap<WidgetId, (Layer, NodeId)>,
    /// Last *painted* frame's `curr`. Only the keys matter for the
    /// rollover diff — the `(Layer, NodeId)` values are stale across
    /// frames and ignored. Same type as `curr` so `std::mem::swap` is
    /// alloc-free.
    prev: FxHashMap<WidgetId, (Layer, NodeId)>,
    /// Diff output: widgets present in `prev` but not in `curr`.
    /// Repopulated by [`Self::rollover`]; consumers iterate via
    /// a shared borrow on the field. Public-in-crate so callers can
    /// hold `&seen.removed` across other shared `&forest` reads — an
    /// accessor returning `&[..]` would tie the returned slice to the
    /// `&mut self` and block those reads. Stored as a `FxHashSet`
    /// (not `Vec`) so consumers that test per-row membership
    /// (`anim`, `text`) get O(1) lookups without rebuilding the set.
    pub(crate) removed: FxHashSet<WidgetId>,
    /// Per-original-id occurrence counter for collision
    /// disambiguation. Bumped inside [`Self::record`] whenever an id
    /// collides; cleared each frame.
    dup: FxHashMap<WidgetId, u32>,
}

impl SeenIds {
    /// Reset per-build state at the top of a frame. Clears the
    /// `curr` recording map + the disambiguation counter.
    /// **Doesn't touch `prev`** — that holds the last *painted*
    /// frame's recording, established by [`Self::rollover`]. A
    /// `run_frame` two-pass discard build calls `pre_record` then
    /// never reaches `rollover`, so `prev` must be preserved across
    /// the discard.
    pub(crate) fn pre_record(&mut self) {
        self.curr.clear();
        self.dup.clear();
    }

    /// Record `element` (about to be opened as `node`) for this
    /// frame, rewriting `element.id` if it collided. Both auto and
    /// explicit collisions disambiguate via an occurrence counter;
    /// explicit collisions additionally return the first-occurrence
    /// `NodeId` so the caller can pair the two colliding nodes for
    /// the debug overlay.
    pub(crate) fn record(
        &mut self,
        element: &mut Element,
        layer: Layer,
        node: NodeId,
    ) -> RecordOutcome {
        use std::collections::hash_map::Entry;
        let id = element.id;
        match self.curr.entry(id) {
            Entry::Vacant(v) => {
                v.insert((layer, node));
                RecordOutcome::Inserted
            }
            Entry::Occupied(o) => {
                let first = *o.get();
                let explicit = matches!(element.slots.id_source(), IdSource::Explicit);
                let counter = self.dup.entry(id).or_insert(0);
                let disambiguated = loop {
                    *counter += 1;
                    let candidate = id.with(*counter);
                    if let Entry::Vacant(v) = self.curr.entry(candidate) {
                        v.insert((layer, node));
                        break candidate;
                    }
                };
                element.id = disambiguated;
                if explicit {
                    RecordOutcome::DisambiguatedExplicit { first }
                } else {
                    RecordOutcome::DisambiguatedAuto
                }
            }
        }
    }

    /// Populate `self.removed` with widgets present in `prev` but
    /// absent from `curr`, then swap `curr → prev` so the next frame
    /// diffs against this one. Returns a borrow of `self.removed`
    /// for callers that want to fan the diff straight into per-widget
    /// caches (text shaper, measure cache, state map, animation,
    /// damage); the field stays populated until the next `rollover`.
    pub(crate) fn rollover(&mut self) -> &FxHashSet<WidgetId> {
        self.removed.clear();
        for wid in self.prev.keys() {
            if !self.curr.contains_key(wid) {
                self.removed.insert(*wid);
            }
        }
        std::mem::swap(&mut self.curr, &mut self.prev);
        self.curr.clear();
        &self.removed
    }
}
