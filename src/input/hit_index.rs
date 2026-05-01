use crate::primitives::{Rect, Sense, TranslateScale, Visibility, WidgetId};
use crate::tree::Tree;
use glam::Vec2;

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
}

impl HitIndex {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            cascades: Vec::new(),
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
    pub(crate) fn rebuild(&mut self, tree: &Tree) {
        self.entries.clear();
        self.cascades.clear();
        let n = tree.nodes.len();
        self.entries.reserve(n);
        self.cascades.reserve(n);

        for node in &tree.nodes {
            let parent = match node.parent {
                Some(p) => self.cascades[p.0 as usize],
                None => Cascade::ROOT_PARENT,
            };

            let me_disabled = parent.disabled || node.element.disabled;
            let me_invisible = parent.invisible || node.element.visibility != Visibility::Visible;

            let parent_t = parent.transform_for_descendants;
            let node_transform = node
                .element
                .extras
                .and_then(|i| tree.node_extras[i as usize].transform);
            let descendant_t = match node_transform {
                Some(t) => parent_t.compose(t),
                None => parent_t,
            };

            let screen_rect = parent_t.apply_rect(node.rect);
            let visible_rect = match parent.clip_for_descendants {
                Some(c) => screen_rect.intersect(c),
                None => screen_rect,
            };
            let descendant_clip = if node.element.clip {
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
                node.element.sense
            };
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
        self.entries
            .iter()
            .find_map(|e| (e.id == id).then_some(e.rect))
    }

    pub(crate) fn contains_id(&self, id: WidgetId) -> bool {
        self.entries.iter().any(|e| e.id == id)
    }
}
