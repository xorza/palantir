//! Per-frame post-arrange state.
//!
//! [`CascadeEngine`](engine::CascadeEngine) owns the walk scratch and
//! updates a retained [`Cascade`]. Paint-only changes repair dirty
//! subtrees in place; geometry or inherited-state changes rebuild all
//! per-tree rows. Downstream phases (damage diff, input hit-test,
//! renderer encoder) take `&Cascade` as their single frozen-state
//! handle.
//!
//! Laid out like `layout`: this root holds the retained product, with
//! the machinery in [`engine`], the row tables in [`entry`], and the
//! paint arena in [`paint`].

#[cfg(feature = "bench")]
pub(crate) mod bench;
pub(crate) mod cascade_input_hash;
pub(crate) mod cascade_key;
pub(crate) mod counters;
pub(crate) mod engine;
pub(crate) mod entry;
pub(crate) mod layer_cascade;
pub(crate) mod paint;
mod paint_rect;

use crate::cascade::cascade_key::CascadeKey;
use crate::cascade::entry::{
    EntryRow, HitRow, HitTargets, PressTargets, RootRow, ScopeRow, TabDirection, TabDomain,
    TabStopRow, WidgetLocation,
};
use crate::cascade::layer_cascade::LayerCascade;
use crate::input::sense::Sense;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdMap};
use crate::scene::endpoint::Endpoint;
use crate::scene::layer::Layer;
use crate::scene::per_layer::PerLayer;
use glam::Vec2;

/// Read-only artifact of `CascadeEngine::run`. Holds per-layer
/// cascade state (per-node rows, subtree rollups, paint arena — see
/// [`LayerCascade`]) plus the [`Self::by_id`] hit-lookup snapshot.
#[derive(Debug, Default)]
pub(crate) struct Cascade {
    pub(crate) layers: PerLayer<LayerCascade>,
    /// One row per recorded node, node-aligned within each layer's
    /// block (`layers[l].entries_base + node.0`). Read only by
    /// per-widget response lookup, which gathers a whole row at one
    /// index — see [`EntryRow`] for why it is AoS. Layers append in
    /// paint order.
    pub(crate) entries: Vec<EntryRow>,
    /// Interactive rows only, in the same paint order as
    /// [`Self::entries`]. Hit tests reverse-scan this table and read
    /// nothing else — see [`HitRow`].
    hits: Vec<HitRow>,
    /// Declared input scopes in record order — see [`ScopeRow`].
    pub(crate) scopes: Vec<ScopeRow>,
    /// Tab stops in record order — see [`TabStopRow`].
    pub(crate) tab_stops: Vec<TabStopRow>,
    /// Layer roots in record order — see [`RootRow`].
    pub(crate) roots: Vec<RootRow>,
    /// `WidgetId → Endpoint` lookup for hit-test consumers
    /// ([`crate::input::input_state::InputState::response_for`], capture / focus
    /// eviction). **Invariant: equals `SeenIds.curr` as observed at
    /// the end of the most recent `CascadeEngine::run`** — a full
    /// rebuild refills it from `seen.curr`, and paint-only runs and
    /// skips retain it because [`Self::key`] includes every widget
    /// identity. The snapshot is required (rather than reading
    /// `seen.curr` directly) because `response_for` is called during
    /// recording, and `SeenIds::pre_record` clears `curr` at the top
    /// of every record pass — `request_relayout`'s second pass needs
    /// to see pass A's entries while its own widgets are still being
    /// recorded into the freshly-cleared `curr`. `seen.prev` is the
    /// wrong fallback: it carries the previous *frame*'s data, not
    /// the previous *pass*'s. Pays one O(N) memcpy per cascade run
    /// on a full rebuild in exchange for not paying an O(N) hashmap
    /// insert per widget.
    pub(crate) by_id: WidgetIdMap<Endpoint>,
    /// The inputs this cascade was built from; `None` before the first
    /// run.
    pub(crate) key: Option<CascadeKey>,
}

impl Cascade {
    /// Where `id` was recorded in the most recent cascade run — the
    /// `(layer, node)` pair the per-node layout columns are indexed by —
    /// or `None` if it isn't in that run.
    ///
    /// The handle half of a lookup whose other half lives on `Layout`
    /// (see [`Layout::scroll_content`](crate::layout::Layout::scroll_content)):
    /// neither table can answer alone, so a caller joins them, and this
    /// is what keeps the map itself out of the join.
    #[inline]
    pub(crate) fn endpoint(&self, id: WidgetId) -> Option<Endpoint> {
        self.by_id.get(&id).copied()
    }

    /// The entry row of the node at `endpoint`. Gated like its one
    /// caller, the development-only collision overlay.
    #[cfg(debug_assertions)]
    #[inline]
    pub(crate) fn entry_at(&self, endpoint: Endpoint) -> &EntryRow {
        &self.entries[(self.layers[endpoint.layer].entries_base + endpoint.node.0) as usize]
    }

    /// Both indexes a widget's per-frame rows are reached by, from one
    /// `by_id` probe. `response_for` needs the entry index (for
    /// [`Cascade::entries`]) *and* the endpoint (for the layout
    /// columns, which are keyed by `(layer, node)`), once per widget per
    /// frame — resolving them separately would double the hash lookups
    /// on that path.
    #[inline]
    pub(crate) fn locate(&self, id: WidgetId) -> Option<WidgetLocation> {
        let endpoint = *self.by_id.get(&id)?;
        Some(WidgetLocation {
            entry_idx: self.layers[endpoint.layer].entries_base + endpoint.node.0,
            endpoint,
        })
    }

    /// True when `descendant`'s most recent record sits inside
    /// `ancestor`'s subtree — same layer, within the ancestor's
    /// pre-order interval `[node, subtree_end)`. Self-inclusive:
    /// `is_within(id, id)` is `true` for any recorded `id`. `false`
    /// when either id wasn't in the most recent cascade run (layers
    /// are separate trees, so a popup is never "within" its anchor).
    pub(crate) fn is_within(&self, descendant: WidgetId, ancestor: WidgetId) -> bool {
        let (Some(d), Some(a)) = (self.by_id.get(&descendant), self.by_id.get(&ancestor)) else {
            return false;
        };
        d.layer == a.layer
            && d.node.0 >= a.node.0
            && d.node.0 < self.layers[a.layer].subtree_ends[a.node.idx()]
    }

    /// Interactive rows under `pos`, topmost first.
    ///
    /// The one reverse scan every hit test is built from: rows are pushed
    /// in paint order, so walking back yields the topmost match first and
    /// the caller stops when it has what it needs.
    #[inline]
    fn hits_under(&self, pos: Vec2) -> impl Iterator<Item = &HitRow> {
        self.hits
            .iter()
            .rev()
            .filter(move |row| row.rect.contains(pos))
    }

    /// One reverse walk that finds the topmost hover, scroll and pinch
    /// targets at once. Used on `PointerMoved` and at `post_record` to
    /// recompute them all in a single pass.
    /// [`Self::hit_test_press`] is the same shape for the press path.
    /// The slots are independent: a `Sense::DRAG | Sense::SCROLL` widget
    /// sits in the hover slot and both scroll slots if it's the topmost
    /// match for each, and the two wheel axes may land on two rows.
    /// Stops as soon as every slot is filled.
    pub(crate) fn hit_test_targets(&self, pos: Vec2) -> HitTargets {
        let mut targets = HitTargets::default();
        for row in self.hits_under(pos) {
            if targets.hover.is_none() && Sense::hovers(row.sense) {
                targets.hover = Some(row.widget_id);
            }
            if targets.scroll.x.is_none() && row.sense.contains(Sense::SCROLL_X) {
                targets.scroll.x = Some(row.widget_id);
            }
            if targets.scroll.y.is_none() && row.sense.contains(Sense::SCROLL_Y) {
                targets.scroll.y = Some(row.widget_id);
            }
            if targets.pinch.is_none() && Sense::pinches(row.sense) {
                targets.pinch = Some(row.widget_id);
            }
            if targets.hover.is_some()
                && targets.scroll.x.is_some()
                && targets.scroll.y.is_some()
                && targets.pinch.is_some()
            {
                break;
            }
        }
        targets
    }

    /// The press target and the focus target in one reverse walk — see
    /// [`PressTargets`]. Same shape as [`Self::hit_test_targets`]: the
    /// predicates are named here rather than passed, because there is one
    /// press path and it wants `Sense::clicks` for the press itself and
    /// `HitRow::focusable` for what the click focuses — different fields,
    /// which is why one filter parameter could not have served both.
    ///
    /// The focus walk normally passes *through* the press target, which
    /// is how clicking a `Button` inside a focusable group focuses the
    /// group. A disabled target ends it instead: the press went no
    /// further, so neither may the focus it would have moved.
    pub(crate) fn hit_test_press(&self, pos: Vec2) -> PressTargets {
        let mut targets = PressTargets::default();
        for row in self.hits_under(pos) {
            if targets.click.is_none() && Sense::clicks(row.sense) {
                targets.click = Some(row.widget_id);
                if row.disabled {
                    break;
                }
            }
            if targets.focus.is_none() && row.focusable {
                targets.focus = Some(row.widget_id);
            }
            if targets.click.is_some() && targets.focus.is_some() {
                break;
            }
        }
        targets
    }

    /// The stop a Tab press moves focus to from `focused`, or `None` when
    /// `domain` holds no stop.
    ///
    /// The stops of `domain` — [`Self::tab_domain`] — in ascending `index`, ties in
    /// record order; `Next` takes the first after `focused` and `Previous`
    /// the last before it, both wrapping. A `focused` outside the domain —
    /// nothing focused, or a field behind an open modal — enters it at the
    /// first stop, or at the last going back.
    ///
    /// One scan over the rows rather than a sort: a stop's order key is
    /// `(index, record position)`, and the answer is a minimum or a
    /// maximum over it.
    pub(crate) fn next_tab_stop(
        &self,
        domain: TabDomain,
        focused: Option<WidgetId>,
        direction: TabDirection,
    ) -> Option<WidgetId> {
        let stops = || {
            self.tab_stops
                .iter()
                .enumerate()
                .filter(move |(_, row)| domain.contains(row))
                .map(|(position, row)| ((row.index, position), row.id))
        };
        let current =
            focused.and_then(|id| stops().find(|&(_, stop)| stop == id).map(|(key, _)| key));
        match direction {
            TabDirection::Next => current
                .and_then(|from| {
                    stops()
                        .filter(|&(key, _)| key > from)
                        .min_by_key(|&(key, _)| key)
                })
                .or_else(|| stops().min_by_key(|&(key, _)| key)),
            TabDirection::Previous => current
                .and_then(|from| {
                    stops()
                        .filter(|&(key, _)| key < from)
                        .max_by_key(|&(key, _)| key)
                })
                .or_else(|| stops().max_by_key(|&(key, _)| key)),
        }
        .map(|(_, id)| id)
    }

    /// The first stop in Tab order recorded under `ancestor`, or `None`
    /// when it holds none or was not recorded.
    pub(crate) fn first_tab_stop_within(&self, ancestor: WidgetId) -> Option<WidgetId> {
        self.tab_stops
            .iter()
            .enumerate()
            .filter(|(_, row)| self.is_within(row.id, ancestor))
            .min_by_key(|&(position, row)| (row.index, position))
            .map(|(_, row)| row.id)
    }

    /// The stops a Tab press from `focused` may reach.
    ///
    /// - A `Menu` root that holds the focus traps it: a menu is raised
    ///   from whatever it sits above, modal included.
    /// - Otherwise the topmost open `Modal` traps it — the last `Modal`
    ///   root recorded — and a focus anywhere else is pulled into it.
    /// - Otherwise a `Popup` root that holds the focus traps it. A popup
    ///   that does not hold it — an autocomplete list under a focused
    ///   field — takes nothing, so Tab moves on through the field's own
    ///   layer, as ARIA's combobox does.
    /// - Otherwise every stop in `Main`.
    pub(crate) fn tab_domain(&self, focused: Option<WidgetId>) -> TabDomain {
        let modal = self
            .roots
            .iter()
            .rev()
            .find(|row| row.layer == Layer::Modal);
        let focus_root = focused.and_then(|id| {
            let layer = self.endpoint(id)?.layer;
            self.roots
                .iter()
                .rev()
                .find(|row| row.layer == layer && self.is_within(id, row.id))
        });
        if let Some(root) = focus_root {
            let traps = match root.layer {
                Layer::Menu => true,
                Layer::Modal => modal == Some(root),
                Layer::Popup => modal.is_none(),
                Layer::Main | Layer::Tooltip | Layer::Debug => false,
            };
            if traps {
                return TabDomain::Root(root.id);
            }
        }
        match modal {
            Some(root) => TabDomain::Root(root.id),
            None => TabDomain::Layer(Layer::Main),
        }
    }
}

// Reached only from the harness's aim assertions and the hit-index test:
// production routes through the two fused walks above, each of which
// answers its whole question in one pass.
#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::cascade::Cascade;
    #[cfg(test)]
    use crate::cascade::LayerCascade;
    #[cfg(test)]
    use crate::common::content_hash::ContentHash;
    use crate::input::sense::Sense;
    #[cfg(test)]
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::identity::widget_id::WidgetId;
    #[cfg(test)]
    use crate::scene::forest::Forest;
    #[cfg(test)]
    use crate::scene::tree::Tree;
    #[cfg(test)]
    use crate::scene::tree::iter::TreeItem;
    #[cfg(test)]
    use crate::scene::tree::node_id::NodeId;
    use glam::Vec2;

    /// One paint row with the widget that owns it — what the damage
    /// oracle diffs between two frames.
    #[cfg(test)]
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(crate) struct OwnedPaint {
        pub(crate) owner: WidgetId,
        pub(crate) screen: Rect,
        pub(crate) hash: ContentHash,
        /// Position in the frame's paint order.
        pub(crate) rank: u32,
    }

    #[cfg(test)]
    impl LayerCascade {
        fn owned_paints_of(&self, tree: &Tree, node: NodeId, out: &mut Vec<OwnedPaint>) {
            let arena = &self.paint_arena;
            let span = arena.node_spans[node.idx()];
            // An invisible node's span is empty, and so is every one
            // under it.
            if span.len == 0 {
                return;
            }
            let owner = tree.records.widget_id()[node.idx()];
            let mut row = span.start as usize;
            let push = |row: usize, out: &mut Vec<OwnedPaint>| {
                let paint = arena.rows[row];
                out.push(OwnedPaint {
                    owner,
                    screen: paint.screen,
                    hash: paint.hash,
                    rank: out.len() as u32,
                });
            };
            if tree.chrome(node).is_some() {
                push(row, out);
                row += 1;
            }
            for item in tree.tree_items(node) {
                match item {
                    TreeItem::ShapeRecord(..) => push(row, out),
                    TreeItem::Child(child) => self.owned_paints_of(tree, child.id, out),
                }
                row += 1;
            }
            debug_assert_eq!(row, span.range().end, "rows out of sync with the span");
        }
    }

    impl Cascade {
        /// Every interactive row's widget, in paint order — the raw
        /// contents of the hit table, for the tests that assert on which
        /// widgets reached it rather than on what a point hits.
        ///
        /// Narrower than the module around it: every caller is an in-crate
        /// unit test, so an `internals` build has none.
        #[cfg(test)]
        pub(crate) fn hit_ids(&self) -> impl Iterator<Item = WidgetId> + '_ {
            self.hits.iter().map(|row| row.widget_id)
        }

        /// Append every row that paints, with the widget that owns it, in
        /// the order the encoder draws them: layer by layer, root by root,
        /// and inside a node its chrome, then each shape and child subtree
        /// in record order. Child markers paint nothing and are left out.
        #[cfg(test)]
        pub(crate) fn owned_paints(&self, forest: &Forest, out: &mut Vec<OwnedPaint>) {
            for (layer, tree) in forest.trees.iter_paint_order() {
                for slot in &tree.roots {
                    self.layers[layer].owned_paints_of(tree, slot.first_node, out);
                }
            }
        }

        /// Assert that this cascade — however it was reached, full or
        /// incremental — equals `cold`, a full rebuild over the same
        /// forest and layout. Compares every column a reader consumes,
        /// per node, so a retained row that went stale is named. The id
        /// lookup is not among them: it snapshots the live seen-id table,
        /// which a rebuild after the frame no longer finds.
        #[cfg(test)]
        pub(crate) fn assert_same_as(&self, cold: &Cascade, forest: &Forest) {
            assert_eq!(self.entries, cold.entries, "entry rows");
            assert_eq!(self.hits, cold.hits, "hit rows");
            let scopes = |cascade: &Cascade| {
                cascade
                    .scopes
                    .iter()
                    .map(|row| (row.layer, row.id, row.filter))
                    .collect::<Vec<_>>()
            };
            assert_eq!(scopes(self), scopes(cold), "scope rows");
            assert_eq!(self.tab_stops, cold.tab_stops, "tab stop rows");
            assert_eq!(self.roots, cold.roots, "root rows");
            for (layer, tree) in forest.trees.iter_paint_order() {
                let (warm, full) = (&self.layers[layer], &cold.layers[layer]);
                for (node, id) in tree.records.widget_id().iter().enumerate() {
                    let at = || format!("{layer:?} node {node} ({id:?})");
                    assert_eq!(
                        warm.cascade_inputs[node],
                        full.cascade_inputs[node],
                        "cascade input of {}",
                        at(),
                    );
                    assert_eq!(
                        warm.subtree_paint_rects[node],
                        full.subtree_paint_rects[node],
                        "subtree paint rect of {}",
                        at(),
                    );
                    assert_eq!(
                        warm.subtree_ends[node],
                        full.subtree_ends[node],
                        "subtree end of {}",
                        at(),
                    );
                    assert_eq!(
                        warm.paint_arena.rows_of(node),
                        full.paint_arena.rows_of(node),
                        "paint rows of {}",
                        at(),
                    );
                }
            }
        }

        /// Topmost entry under `pos` whose `Sense` passes `filter`.
        pub(crate) fn hit_test(
            &self,
            pos: Vec2,
            filter: impl Fn(Sense) -> bool,
        ) -> Option<WidgetId> {
            self.hits_under(pos)
                .find(|row| filter(row.sense))
                .map(|row| row.widget_id)
        }
    }
}

#[cfg(test)]
mod tests;
