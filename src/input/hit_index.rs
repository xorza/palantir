use crate::layout::LayoutResult;
use crate::primitives::{Rect, Sense, TranslateScale, WidgetId};
use crate::tree::{NodeId, Tree};
use glam::Vec2;
use std::collections::HashMap;

/// One widget's hit-test entry from last frame: identity, screen-space rect
/// (clipped by ancestors), and effective `Sense` (with disabled/visibility
/// cascade applied).
#[derive(Clone, Copy, Debug)]
struct HitEntry {
    id: WidgetId,
    rect: Rect,
    sense: Sense,
}

/// Per-node ancestor-cascade state computed during `HitIndex::rebuild`. A
/// single struct keeps the four cascades indexed in lockstep — adding a new
/// one is a field, not another parallel `Vec`. None of these survive across
/// frames in a meaningful sense; they only persist as scratch to skip
/// per-frame allocations.
#[derive(Clone, Copy, Debug)]
struct Cascade {
    disabled: bool,
    invisible: bool,
    /// Cumulative transform that applies to descendants (NOT to this node's
    /// own rect — that uses the parent's entry).
    transform_for_descendants: TranslateScale,
    /// Clip rect inherited by descendants, in screen space. `None` = no clip.
    clip_for_descendants: Option<Rect>,
}

impl Cascade {
    const ROOT_PARENT: Self = Self {
        disabled: false,
        invisible: false,
        transform_for_descendants: TranslateScale::IDENTITY,
        clip_for_descendants: None,
    };
}

/// Pre-order snapshot of the just-arranged tree, in the form needed for
/// hit-testing: each node's screen-space rect (clipped by ancestors), its
/// effective `Sense` (cascading disabled/visibility), and an ordered list to
/// reverse-iterate for topmost-first lookups.
///
/// Rebuilt every `Ui::end_frame` from `&Tree`. Owns the cascade scratch so
/// rebuild is alloc-free in steady state. Read-only after rebuild.
pub(crate) struct HitIndex {
    entries: Vec<HitEntry>,
    cascades: Vec<Cascade>,
    /// `WidgetId → entries[idx]`. Populated alongside `entries` during
    /// `rebuild` so `rect_for` / `contains_id` are O(1) instead of O(n) —
    /// these run on every input event while an active widget is captured.
    /// Capacity is reused across frames; uniqueness of ids is enforced by
    /// `Ui::node`'s release assert.
    by_id: HashMap<WidgetId, u32>,
}

impl HitIndex {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            cascades: Vec::new(),
            by_id: HashMap::new(),
        }
    }

    /// Walk `tree.nodes` in storage order (== pre-order, since recording is
    /// depth-first) and produce one `HitEntry` per node, threading four
    /// ancestor cascades through `Cascade[parent_idx]`:
    ///
    /// - **`disabled`** / **`invisible`**: any ancestor (or self) flagged
    ///   forces effective `Sense::NONE`.
    /// - **`transform`**: parent's cumulative transform places this node's
    ///   own rect into screen space; this node's *own* transform contributes
    ///   only to descendants (matching the encoder's emit order).
    /// - **`clip`**: clipping ancestors bound the visible/hit-testable area
    ///   of descendants. Stored in screen space so intersection composes
    ///   directly with transformed rects.
    pub(crate) fn rebuild(&mut self, tree: &Tree, layout: &LayoutResult) {
        self.entries.clear();
        self.cascades.clear();
        self.by_id.clear();
        let n = tree.node_count();
        self.entries.reserve(n);
        self.cascades.reserve(n);
        self.by_id.reserve(n);

        for (i, node) in tree.nodes_iter().enumerate() {
            let parent = match node.parent {
                Some(p) => self.cascades[p.0 as usize],
                None => Cascade::ROOT_PARENT,
            };

            let me_disabled = parent.disabled || node.element.flags.is_disabled();
            let me_invisible = parent.invisible || !node.element.flags.is_visible();

            let parent_t = parent.transform_for_descendants;
            let node_transform = tree.read_extras(NodeId(i as u32)).transform;
            let descendant_t = match node_transform {
                Some(t) => parent_t.compose(t),
                None => parent_t,
            };

            let screen_rect = parent_t.apply_rect(layout.rect(NodeId(i as u32)));
            let visible_rect = match parent.clip_for_descendants {
                Some(c) => screen_rect.intersect(c),
                None => screen_rect,
            };
            let descendant_clip = if node.element.flags.is_clip() {
                Some(match parent.clip_for_descendants {
                    Some(c) => screen_rect.intersect(c),
                    None => screen_rect,
                })
            } else {
                parent.clip_for_descendants
            };

            self.cascades.push(Cascade {
                disabled: me_disabled,
                invisible: me_invisible,
                transform_for_descendants: descendant_t,
                clip_for_descendants: descendant_clip,
            });

            let sense = if me_disabled || me_invisible {
                Sense::NONE
            } else {
                node.element.flags.sense()
            };
            self.by_id
                .insert(node.element.id, self.entries.len() as u32);
            self.entries.push(HitEntry {
                id: node.element.id,
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
