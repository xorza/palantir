//! Per-frame post-arrange state.
//!
//! [`CascadeEngine`](engine::CascadeEngine) updates a retained [`Cascade`]: while structure holds, geometry and paint changes refresh moved rows in place; a structural change rebuilds all rows. Downstream phases take `&Cascade` as their frozen-state handle.

#[cfg(feature = "internals")]
pub(crate) mod bench;
pub(crate) mod cascade_input_hash;
pub(crate) mod cascade_key;
pub(crate) mod cascade_source;
pub(crate) mod counters;
pub(crate) mod engine;
pub(crate) mod paint;
mod paint_rect;
pub(crate) mod pane_row;
pub(crate) mod plane_cascade;
pub(crate) mod rows;

use crate::cascade::cascade_key::CascadeKey;
use crate::cascade::plane_cascade::PlaneCascade;
use crate::cascade::rows::arrow_group_row::ArrowGroupRow;
use crate::cascade::rows::entry_row::EntryRow;
use crate::cascade::rows::hit_row::HitRow;
use crate::cascade::rows::root_row::RootRow;
use crate::cascade::rows::scope_row::ScopeRow;
use crate::cascade::rows::tab_stop_row::TabStopRow;
use crate::input::scroll_targets::ScrollTargets;
use crate::input::sense::Sense;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdMap};
use crate::scene::endpoint::Endpoint;
use crate::scene::layer::Layer;
use crate::scene::per_plane::PerPlane;
use crate::scene::plane::Plane;
use glam::Vec2;

/// Read-only artifact of `CascadeEngine::run`: per-plane state ([`PlaneCascade`]) plus the [`Self::by_id`] snapshot.
#[derive(Debug, Default)]
pub(crate) struct Cascade {
    pub(crate) planes: PerPlane<PlaneCascade>,
    /// One row per recorded node, node-aligned within each plane's block (`planes[p].entries_base + node.0`); planes append in paint order. See [`EntryRow`].
    pub(crate) entries: Vec<EntryRow>,
    /// Interactive rows only, in paint order, reverse-scanned by hit tests; see [`HitRow`].
    hits: Vec<HitRow>,
    /// Declared input scopes in paint order — see [`ScopeRow`].
    pub(crate) scopes: Vec<ScopeRow>,
    /// Tab stops in paint order — see [`TabStopRow`].
    pub(crate) tab_stops: Vec<TabStopRow>,
    /// Plane roots in paint order — see [`RootRow`].
    pub(crate) roots: Vec<RootRow>,
    /// Arrow groups in record order — see [`ArrowGroupRow`].
    pub(crate) arrow_groups: Vec<ArrowGroupRow>,
    /// `WidgetId → Endpoint` lookup. **Invariant: equals the recorded entries of `SeenIds.cur` at the end of the latest `CascadeEngine::run`.** A snapshot because `response_for` runs mid-record, after `SeenIds::pre_record` cleared `cur`, and a relayout's second pass must still see pass A's entries; `seen.prev` holds the previous frame, not pass.
    pub(crate) by_id: WidgetIdMap<Endpoint>,
    /// The inputs this cascade was built from; `None` before the first run.
    pub(crate) key: Option<CascadeKey>,
}

/// Where one widget's per-frame rows live: the flat
/// [`Cascade::entries`] index plus the `(layer, node)`
/// [`Endpoint`] the layout columns are keyed by. From
/// [`Cascade::locate`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct WidgetLocation {
    pub(crate) entry_idx: u32,
    pub(crate) endpoint: Endpoint,
}

/// Which way a Tab press moves focus: Tab forward, Shift+Tab back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabDirection {
    Next,
    Prev,
}

/// The stops one Tab press may reach; see
/// [`Cascade::next_tab_stop`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabDomain {
    /// The stops recorded under one overlay root.
    Root(WidgetId),
    /// Every stop of ordinary content.
    Main,
    /// The stops recorded under one arrow group.
    Group(WidgetId),
}

/// What a press lands on: the topmost clickable row under the point and the
/// topmost focusable one, from a single reverse scan. The answers are
/// independent (clicking a `Button` must not steal focus from a `TextEdit`)
/// except that a disabled row absorbs the press, so `focus` is whatever sat
/// above it, `None` if nothing did.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct PressTargets {
    pub(crate) click: Option<WidgetId>,
    pub(crate) focus: Option<WidgetId>,
}

/// Topmost interactive row under a point for each independent sense filter
/// (hover, each wheel axis, pinch), from one reverse scan.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct HitTargets {
    pub(crate) hover: Option<WidgetId>,
    pub(crate) scroll: ScrollTargets,
    pub(crate) pinch: Option<WidgetId>,
}

impl Cascade {
    /// Every interactive row in paint order, for the widget inspection.
    pub(crate) fn hit_rows(&self) -> &[HitRow] {
        &self.hits
    }

    /// Where `id` was recorded in the latest run, as the `(plane, node)` pair layout columns are indexed by, or `None`.
    #[inline]
    pub(crate) fn endpoint(&self, id: WidgetId) -> Option<Endpoint> {
        self.by_id.get(&id).copied()
    }

    /// The entry row at `endpoint`. Gated like its one caller, the development-only collision overlay.
    #[cfg(debug_assertions)]
    #[inline]
    pub(crate) fn entry_at(&self, endpoint: Endpoint) -> &EntryRow {
        &self.entries[(self.planes[endpoint.plane].entries_base + endpoint.node.0) as usize]
    }

    /// Both the entry index and the endpoint, from one `by_id` probe.
    #[inline]
    pub(crate) fn locate(&self, id: WidgetId) -> Option<WidgetLocation> {
        Some(self.location(*self.by_id.get(&id)?))
    }

    /// [`Self::locate`] for an endpoint already known, with no probe.
    #[inline]
    pub(crate) fn location(&self, endpoint: Endpoint) -> WidgetLocation {
        WidgetLocation {
            entry_idx: self.planes[endpoint.plane].entries_base + endpoint.node.0,
            endpoint,
        }
    }

    /// True when `descendant` sits inside `ancestor`'s subtree: same plane, within `[node, subtree_end)`. Self-inclusive; `false` if either id is missing from the latest run.
    pub(crate) fn is_within(&self, descendant: WidgetId, ancestor: WidgetId) -> bool {
        let (Some(d), Some(a)) = (self.by_id.get(&descendant), self.by_id.get(&ancestor)) else {
            return false;
        };
        d.plane == a.plane
            && d.node.0 >= a.node.0
            && d.node.0 < self.planes[a.plane].subtree_ends[a.node.idx()]
    }

    /// Interactive rows under `pos`, topmost first, by reverse scan of the paint-ordered table.
    #[inline]
    fn hits_under(&self, pos: Vec2) -> impl Iterator<Item = &HitRow> {
        self.hits
            .iter()
            .rev()
            .filter(move |row| row.rect.contains(pos))
    }

    /// One reverse walk finding the topmost hover, scroll and pinch targets; [`Self::hit_test_press`] is the press twin. Slots are independent, and the walk stops once all are filled.
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

    /// The press and focus targets in one reverse walk; see [`PressTargets`]. The predicates differ (`Sense::clicks` vs `HitRow::focusable`). Focus passes through the press target to a focusable ancestor, but a disabled target ends both.
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

    /// The stop a Tab press moves focus to from `focused`, or `None` when `domain` holds no stop. Order is ascending `idx`, ties in record order; `Next`/`Prev` wrap. A `focused` outside the domain enters at the first stop, or the last going back.
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
                .filter(move |(_, row)| self.in_domain(domain, row))
                .map(|(pos, row)| ((row.idx, pos), row.id))
        };
        let cur = focused.and_then(|id| stops().find(|&(_, stop)| stop == id).map(|(key, _)| key));
        match direction {
            TabDirection::Next => cur
                .and_then(|from| {
                    stops()
                        .filter(|&(key, _)| key > from)
                        .min_by_key(|&(key, _)| key)
                })
                .or_else(|| stops().min_by_key(|&(key, _)| key)),
            TabDirection::Prev => cur
                .and_then(|from| {
                    stops()
                        .filter(|&(key, _)| key < from)
                        .max_by_key(|&(key, _)| key)
                })
                .or_else(|| stops().max_by_key(|&(key, _)| key)),
        }
        .map(|(_, id)| id)
    }

    /// Whether `row` is one of `domain`'s stops.
    fn in_domain(&self, domain: TabDomain, row: &TabStopRow) -> bool {
        match domain {
            TabDomain::Root(root) => row.root == root,
            TabDomain::Main => row.plane == Plane::MAIN,
            TabDomain::Group(group) => self.is_within(row.id, group),
        }
    }

    /// The plane root `id` was recorded under, or `None` when `id` is not in the latest run.
    fn root_of(&self, id: WidgetId) -> Option<&RootRow> {
        let plane = self.endpoint(id)?.plane;
        self.roots
            .iter()
            .rev()
            .find(|row| row.plane == plane && self.is_within(id, row.id))
    }

    /// The overlay `id` sits in: its plane root, unless that root is ordinary content.
    pub(crate) fn overlay_root_of(&self, id: WidgetId) -> Option<WidgetId> {
        self.root_of(id)
            .filter(|row| row.layer != Layer::Main)
            .map(|row| row.id)
    }

    /// The topmost open dialog's root.
    pub(crate) fn modal_root(&self) -> Option<&RootRow> {
        self.roots
            .iter()
            .rev()
            .find(|row| row.layer == Layer::Modal)
    }

    /// Whether `id` can hold focus: recorded in the latest run, focusable, and neither disabled
    /// nor invisible.
    pub(crate) fn is_focusable(&self, id: WidgetId) -> bool {
        self.endpoint(id)
            .and_then(|at| self.planes[at.plane].hit_rows[at.node.idx()])
            .is_some_and(|hit| self.hits[hit.idx()].focusable)
    }

    /// The innermost arrow group `focused` sits in, or `None`; groups are in pre-order, so the last match.
    pub(crate) fn arrow_group_of(&self, focused: WidgetId) -> Option<ArrowGroupRow> {
        self.arrow_groups
            .iter()
            .rev()
            .find(|row| self.is_within(focused, row.id))
            .copied()
    }

    /// The first stop in Tab order under `ancestor`, or `None`.
    pub(crate) fn first_tab_stop_within(&self, ancestor: WidgetId) -> Option<WidgetId> {
        self.tab_stops
            .iter()
            .enumerate()
            .filter(|(_, row)| self.is_within(row.id, ancestor))
            .min_by_key(|&(pos, row)| (row.idx, pos))
            .map(|(_, row)| row.id)
    }

    /// The stops a Tab press from `focused` may reach, by priority:
    ///
    /// - A `Menu` root holding the focus traps it.
    /// - Otherwise the topmost open `Modal` traps it, pulling focus in.
    /// - Otherwise a `Popup` root holding the focus traps it, unless a dialog sits above it; one that does not hold focus (an autocomplete list) takes nothing.
    /// - Otherwise every stop in `Main`.
    ///
    /// Each root is read by the layer it asked for, not its plane's: a dropdown nested in a dialog records on a `Modal` plane and still traps as a popup.
    pub(crate) fn tab_domain(&self, focused: Option<WidgetId>) -> TabDomain {
        let modal = self.modal_root();
        if let Some(root) = focused.and_then(|id| self.root_of(id)) {
            let traps = match root.layer {
                Layer::Menu => true,
                Layer::Modal => modal == Some(root),
                Layer::Popup => modal.is_none_or(|modal| modal.plane < root.plane),
                Layer::Main | Layer::Tooltip | Layer::Debug => false,
            };
            if traps {
                return TabDomain::Root(root.id);
            }
        }
        match modal {
            Some(root) => TabDomain::Root(root.id),
            None => TabDomain::Main,
        }
    }
}

/// What test harness assertions read; production uses the fused walks above.
#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::cascade::Cascade;
    #[cfg(test)]
    use crate::common::content_hash::ContentHash;
    use crate::input::sense::Sense;
    #[cfg(test)]
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::identity::widget_id::WidgetId;
    #[cfg(test)]
    use crate::scene::forest::Forest;
    use glam::Vec2;

    /// One paint row with its owning widget; what the damage oracle diffs.
    #[cfg(test)]
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(crate) struct OwnedPaint {
        pub(crate) owner: WidgetId,
        pub(crate) screen: Rect,
        pub(crate) hash: ContentHash,
        /// Position in the frame's paint order.
        pub(crate) rank: u32,
    }

    impl Cascade {
        /// Every interactive row's widget in paint order, for tests asserting what reached the hit table.
        #[cfg(test)]
        pub(crate) fn hit_ids(&self) -> impl Iterator<Item = WidgetId> + '_ {
            self.hits.iter().map(|row| row.widget_id)
        }

        /// Append every painting row with its widget in encoder draw order. Child markers are left out.
        #[cfg(test)]
        pub(crate) fn owned_paints(&self, forest: &Forest, out: &mut Vec<OwnedPaint>) {
            for (plane, tree) in forest.trees.iter_paint_order() {
                for slot in &tree.roots {
                    self.planes[plane].owned_paints_of(tree, slot.first_node, out);
                }
            }
        }

        /// Assert this cascade equals `cold`, a full rebuild over the same forest and layout, per node and column. The id lookup is excluded since it snapshots live seen ids.
        #[cfg(test)]
        pub(crate) fn assert_same_as(&self, cold: &Self, forest: &Forest) {
            assert_eq!(self.entries, cold.entries, "entry rows");
            assert_eq!(self.hits, cold.hits, "hit rows");
            let scopes = |cascade: &Self| {
                cascade
                    .scopes
                    .iter()
                    .map(|row| (row.plane, row.id, row.filter))
                    .collect::<Vec<_>>()
            };
            assert_eq!(scopes(self), scopes(cold), "scope rows");
            assert_eq!(self.tab_stops, cold.tab_stops, "tab stop rows");
            assert_eq!(self.roots, cold.roots, "root rows");
            assert_eq!(self.arrow_groups, cold.arrow_groups, "arrow group rows");
            for (plane, tree) in forest.trees.iter_paint_order() {
                let (warm, full) = (&self.planes[plane], &cold.planes[plane]);
                for (node, id) in tree.records.widget_id().iter().enumerate() {
                    let at = || format!("{plane:?} node {node} ({id:?})");
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
                        warm.paint_rects[node],
                        full.paint_rects[node],
                        "own paint rect of {}",
                        at(),
                    );
                    assert_eq!(
                        warm.hit_rows[node],
                        full.hit_rows[node],
                        "hit row of {}",
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
