use crate::cascade::Cascades;
use crate::primitives::{Rect, Sense, WidgetId};
use crate::tree::Tree;
use glam::Vec2;
use rustc_hash::FxHashMap;

/// One widget's hit-test entry from last frame: identity, screen-space rect
/// (clipped by ancestors), and effective `Sense` (with disabled/visibility
/// cascade applied).
#[derive(Clone, Copy, Debug)]
struct HitEntry {
    id: WidgetId,
    rect: Rect,
    sense: Sense,
}

/// Pre-order snapshot of the just-arranged tree, in the form needed for
/// hit-testing: each node's screen-space rect (clipped by ancestors), its
/// effective `Sense` (cascading disabled/visibility), and an ordered list to
/// reverse-iterate for topmost-first lookups.
///
/// Rebuilt every `Ui::end_frame` from the shared `Cascades` table. Owns no
/// cascade scratch of its own — that lives in `Cascades` so the encoder and
/// hit index can't drift.
pub(crate) struct HitIndex {
    entries: Vec<HitEntry>,
    /// `WidgetId → entries[idx]`. Populated alongside `entries` during
    /// `rebuild` so `rect_for` / `contains_id` are O(1) instead of O(n) —
    /// these run on every input event while an active widget is captured.
    /// Capacity is reused across frames; uniqueness of ids is enforced by
    /// `Ui::node`'s release assert.
    by_id: FxHashMap<WidgetId, u32>,
}

impl HitIndex {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            by_id: FxHashMap::default(),
        }
    }

    /// Walk `tree.nodes` in storage order alongside the precomputed
    /// `Cascades` to produce one `HitEntry` per node. Cascade rules
    /// (disabled / invisible / clip / transform) live entirely in
    /// `Cascades`; this method only flattens to the per-id form hit-testing
    /// needs.
    pub(crate) fn rebuild(&mut self, tree: &Tree, cascades: &Cascades) {
        self.entries.clear();
        self.by_id.clear();
        let n = tree.node_count();
        self.entries.reserve(n);
        self.by_id.reserve(n);

        let paint = tree.paints();
        let widget_ids = tree.widget_ids();
        let rows = cascades.rows();
        for i in 0..n {
            let c = rows[i];

            let visible_rect = match c.clip {
                Some(cl) => c.screen_rect.intersect(cl),
                None => c.screen_rect,
            };
            let sense = if c.disabled || c.invisible {
                Sense::NONE
            } else {
                paint[i].attrs.sense()
            };

            let widget_id = widget_ids[i];
            self.by_id.insert(widget_id, self.entries.len() as u32);
            self.entries.push(HitEntry {
                id: widget_id,
                rect: visible_rect,
                sense,
            });
        }
    }

    /// Reverse-iter entries → topmost-first under pre-order paint walk.
    /// `filter` decides which `Sense` values participate (hoverable for hover,
    /// clickable for press/release).
    pub(crate) fn hit_test(&self, pos: Vec2, filter: impl Fn(Sense) -> bool) -> Option<WidgetId> {
        for e in self.entries.iter().rev() {
            if filter(e.sense) && e.rect.contains(pos) {
                return Some(e.id);
            }
        }
        None
    }

    pub(crate) fn rect_for(&self, id: WidgetId) -> Option<Rect> {
        self.by_id.get(&id).map(|&i| self.entries[i as usize].rect)
    }

    pub(crate) fn contains_id(&self, id: WidgetId) -> bool {
        self.by_id.contains_key(&id)
    }
}
