//! The dock's persisted arrangement: a binary split tree whose leaves
//! are tab groups, plus the six ops that mutate it.
//!
//! **Flat storage.** The tree lives in one `Vec<DockNode<T>>` with
//! [`NodeIdx`] children — no per-node box. The vector is kept
//! *canonical*: pre-order from the root at slot 0, with no dead slots,
//! because every structural op ends by re-packing. That makes `Vec`
//! equality structural equality — which is what lets an undo layer diff
//! two snapshots for a no-op — and makes group iteration a plain vector
//! scan in left-to-right pane order.
//!
//! Invariants, checked by [`DockState::validate`]:
//! - the vector is canonical pre-order, fully reachable from slot 0;
//! - some group holds the pinned tab;
//! - no group is empty, no tab appears twice, group ids are unique,
//!   each `active` is in range, `focused` names a live group, and every
//!   ratio stays inside the clamp.

use std::hash::Hash;

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::dock::allowed_splits::AllowedSplits;
use crate::widgets::dock::dock_node::{DockNode, DockSplit, NodeIdx};
use crate::widgets::dock::dock_op::{DockDrop, DockOp};
use crate::widgets::dock::dock_path::DockPath;
use crate::widgets::dock::dock_tab::DockTab;
use crate::widgets::dock::error::DockError;
use crate::widgets::dock::pane_geometry::{DropTarget, PaneGeometry};
use crate::widgets::dock::split_side::SplitSide;
use crate::widgets::dock::tab_drag::TabDrag;
use crate::widgets::dock::tab_group::{TabGroup, TabGroupId};
use crate::widgets::tabs::tab_strip::TabStrip;

/// Split-ratio clamp: neither pane can be squeezed below a tenth of the
/// split, so a divider cannot be dragged into an unrecoverable sliver.
const RATIO_MIN: f32 = 0.1;
const RATIO_MAX: f32 = 0.9;

/// Most nested splits allowed on any root-to-leaf chain — up to 16
/// panes. Keeps the arrangement usable and every split address
/// comfortably inside a [`DockPath`].
const DEFAULT_MAX_DEPTH: u32 = 4;

fn default_max_depth() -> u32 {
    DEFAULT_MAX_DEPTH
}

/// A tab's position in the tree: which group holds it, and where in that
/// group's strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabAddress {
    pub group: TabGroupId,
    pub index: usize,
}

/// The whole pane arrangement — the flat split tree, which group holds
/// focus, and the policy the ops enforce.
///
/// The application owns one of these per tab domain and persists it. The
/// widget never mutates it: [`DockView`](crate::DockView) reads it and
/// emits [`DockOp`]s, and [`Self::apply`] is the one place a mutation
/// happens. That is what lets an application route dock ops through the
/// same queue as its own edits, keep them out of undo, and validate the
/// tree before a save.
///
/// ```
/// # use palantir::{DockOp, DockState};
/// # #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// # enum Tab { Main, Console }
/// let mut dock = DockState::new("app.dock", Tab::Main);
/// dock.apply(DockOp::OpenTab { tab: Tab::Console });
/// assert_eq!(dock.groups().count(), 1);
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockState<T> {
    /// Canonical pre-order — see the module doc. Private so every
    /// structural mutation goes through an op that re-packs.
    nodes: Vec<DockNode<T>>,
    focused: TabGroupId,
    /// Next group id to mint. A counter rather than a random source, so
    /// two states built by the same calls compare equal.
    next_group: u64,
    /// The tab that never closes, so the tree is never empty.
    pinned: T,
    /// Scopes every widget id this dock derives. Eight bytes, so two
    /// docks in one application cannot collide.
    seed: u64,
    /// Policy, not document: not serialised. A state read back from a
    /// file carries the default, so an application that changes the cap
    /// applies the change after loading.
    #[serde(skip, default = "default_max_depth")]
    max_depth: u32,
    /// Policy, not document — see [`Self::max_depth`].
    #[serde(skip, default)]
    allowed_splits: AllowedSplits,
}

impl<T: DockTab> DockState<T> {
    /// The root node's index — always slot 0 in the canonical order.
    pub const ROOT: NodeIdx = NodeIdx(0);

    /// Smallest share a split will give either pane, so a divider
    /// cannot be dragged into an unrecoverable sliver.
    pub const RATIO_MIN: f32 = RATIO_MIN;
    /// Largest share a split will give the first pane — the mirror of
    /// [`Self::RATIO_MIN`].
    pub const RATIO_MAX: f32 = RATIO_MAX;

    /// A dock holding one pane, showing `pinned`.
    ///
    /// `seed` scopes every widget id this dock derives, so two docks in
    /// one application never collide. Use one stable string per tab
    /// domain.
    ///
    /// `pinned` is the tab that refuses to close. Its group therefore
    /// cannot collapse, the root always survives, and the arrangement
    /// has no empty state at all — which removes a whole class of
    /// empty-state bugs, at the cost of forbidding a dock that can be
    /// closed down to nothing.
    pub fn new(seed: impl Hash, pinned: T) -> Self {
        let primary = TabGroup {
            id: TabGroupId(0),
            tabs: vec![pinned],
            active: 0,
        };
        Self {
            focused: primary.id,
            nodes: vec![DockNode::Group(primary)],
            next_group: 1,
            pinned,
            seed: WidgetId::from_hash(seed).0,
            max_depth: DEFAULT_MAX_DEPTH,
            allowed_splits: AllowedSplits::All,
        }
    }

    /// Cap the split nesting on any root-to-leaf chain. Default 4 — up
    /// to 16 panes.
    ///
    /// On the state rather than on the view because the *model* enforces
    /// it: [`Self::apply`] refuses a deeper split and [`Self::validate`]
    /// rejects a tree that holds one, so a second copy on the widget
    /// could only fall out of step with this one.
    ///
    /// # Panics
    ///
    /// Panics if `depth` exceeds what a [`DockPath`] can address.
    pub fn max_depth(mut self, depth: u32) -> Self {
        assert!(
            depth <= DockPath::CAPACITY,
            "max_depth {depth} exceeds the {} levels a DockPath addresses",
            DockPath::CAPACITY,
        );
        self.max_depth = depth;
        self
    }

    /// Which split directions a drag offers. Default
    /// [`AllowedSplits::All`]. On the state for the same reason as
    /// [`Self::max_depth`].
    pub fn allowed_splits(mut self, allowed: AllowedSplits) -> Self {
        self.allowed_splits = allowed;
        self
    }

    /// The tab that refuses to close.
    pub fn pinned(&self) -> T {
        self.pinned
    }

    /// The group keyboard shortcuts and newly opened tabs go to.
    pub fn focused(&self) -> TabGroupId {
        self.focused
    }

    /// The node at `idx` — the record walk follows [`DockSplit`]'s child
    /// indices through this.
    pub fn node(&self, idx: NodeIdx) -> &DockNode<T> {
        &self.nodes[idx.usize()]
    }

    /// The leaf groups in left-to-right, top-to-bottom pane order — in
    /// canonical pre-order storage that is simply vector order.
    pub fn groups(&self) -> impl Iterator<Item = &TabGroup<T>> {
        self.nodes.iter().filter_map(|n| match n {
            DockNode::Group(g) => Some(g),
            DockNode::Split(_) => None,
        })
    }

    /// Every open tab across every group, in [`Self::groups`] order.
    pub fn all_tabs(&self) -> impl Iterator<Item = T> + '_ {
        self.groups().flat_map(|g| g.tabs.iter().copied())
    }

    /// What each pane is showing — one tab per group, in
    /// [`Self::groups`] order.
    pub fn active_tabs(&self) -> impl Iterator<Item = T> + '_ {
        self.groups().map(TabGroup::active_tab)
    }

    /// The group holding the pinned tab — the one pane that always
    /// exists.
    pub fn primary(&self) -> &TabGroup<T> {
        self.groups()
            .find(|g| g.tabs.contains(&self.pinned))
            .expect("a group holds the pinned tab")
    }

    pub fn find_tab(&self, tab: T) -> Option<TabAddress> {
        self.groups().find_map(|g| {
            g.tabs
                .iter()
                .position(|t| *t == tab)
                .map(|index| TabAddress { group: g.id, index })
        })
    }

    /// One group by id, or `None` once it has collapsed.
    pub fn group(&self, id: TabGroupId) -> Option<&TabGroup<T>> {
        self.groups().find(|g| g.id == id)
    }

    fn group_mut(&mut self, id: TabGroupId) -> Option<&mut TabGroup<T>> {
        self.nodes.iter_mut().find_map(|n| match n {
            DockNode::Group(g) if g.id == id => Some(g),
            _ => None,
        })
    }

    /// Execute one [`DockOp`] — the dispatch behind every recorded
    /// mutation.
    pub fn apply(&mut self, op: DockOp<T>) {
        match op {
            DockOp::ActivateTab { tab } => self.activate(tab),
            DockOp::OpenTab { tab } => self.open_tab(tab),
            DockOp::CloseTab { tab } => self.close_tab(tab),
            DockOp::MoveTab { tab, to } => self.move_tab(tab, to),
            DockOp::SetRatio { split, ratio } => self.set_ratio(split, ratio),
            DockOp::FocusPane { group } => self.focus(group),
        }
    }

    /// Move focus onto `group` — the pane a press landed in.
    ///
    /// A group that has gone since the press no-ops, like every other op
    /// fed a stale address: storing a dead id would strand `focused` and
    /// fail [`Self::validate`] at the next save.
    fn focus(&mut self, group: TabGroupId) {
        if self.group(group).is_some() {
            self.focused = group;
        }
    }

    /// Add `tab` to the focused group unless it is already open
    /// somewhere, then activate it — which also focuses whichever pane
    /// ended up holding it.
    fn open_tab(&mut self, tab: T) {
        self.find_or_insert(tab, self.focused);
        self.activate(tab);
    }

    /// Make `tab` the visible one in whichever group holds it, and focus
    /// that group. A tab that has since closed no-ops.
    fn activate(&mut self, tab: T) {
        let Some(TabAddress { group, index }) = self.find_tab(tab) else {
            return;
        };
        self.group_mut(group)
            .expect("find_tab resolved a live group")
            .active = index;
        self.focused = group;
    }

    /// Append `tab` to `group`'s strip unless it is already open
    /// somewhere — the half of [`DockOp::OpenTab`] that puts the tab in
    /// the tree, without the activation that follows.
    ///
    /// Unlike the queued ops this is a direct call whose callers name a
    /// group they hold live, so a dead id is a logic error rather than
    /// tolerable staleness.
    pub fn find_or_insert(&mut self, tab: T, group: TabGroupId) {
        if self.find_tab(tab).is_none() {
            self.group_mut(group)
                .expect("insert target group exists")
                .tabs
                .push(tab);
        }
    }

    /// Close `tab` wherever it sits. The pinned tab never closes. A
    /// group emptied by the close collapses out of the tree; a vanished
    /// focus falls back to the primary group.
    fn close_tab(&mut self, tab: T) {
        if tab == self.pinned {
            return;
        }
        let Some(TabAddress { group, index }) = self.find_tab(tab) else {
            return;
        };
        self.group_mut(group)
            .expect("find_tab resolved a live group")
            .remove_tab(index);
        self.normalize();
    }

    /// Move `tab` to `drop`, collapsing whatever its departure empties.
    ///
    /// An `Into` index addresses the target strip *as the caller saw it*
    /// — before the move — so a reorder inside one group lands exactly
    /// where the drop-zone arithmetic over the visible chips said,
    /// despite the tab's own removal shifting the slots. The destination
    /// group takes the tab as its active one and gains focus.
    ///
    /// Degenerate moves — splitting a lone tab off its own group, which
    /// would empty that group and re-split its collapsed remains — leave
    /// the tree unchanged.
    fn move_tab(&mut self, tab: T, drop: DockDrop) {
        let Some(source) = self.find_tab(tab) else {
            return;
        };
        let target = match drop {
            DockDrop::Into { group, .. } | DockDrop::Split { group, .. } => group,
        };
        if self.group(target).is_none() {
            return;
        }
        let source_len = self.group(source.group).expect("source exists").tabs.len();
        if source.group == target && source_len == 1 {
            return;
        }
        // Depth cap, checked before any mutation so a refused split
        // cannot lose the already-removed tab.
        if let DockDrop::Split { side, .. } = drop
            && (!self.can_split(target) || !self.allowed_splits.allows(side))
        {
            return;
        }

        self.group_mut(source.group)
            .expect("source exists")
            .remove_tab(source.index);

        match drop {
            DockDrop::Into { group, index } => {
                // `index` addresses the strip as the caller saw it. A
                // rightward move inside one group must compensate for
                // its own removal.
                let index = if group == source.group && index > source.index {
                    index - 1
                } else {
                    index
                };
                let g = self.group_mut(group).expect("target exists");
                let index = index.min(g.tabs.len());
                g.tabs.insert(index, tab);
                g.active = index;
                self.focused = group;
            }
            DockDrop::Split { group, side } => {
                let new_group = TabGroup {
                    id: TabGroupId(self.next_group),
                    tabs: vec![tab],
                    active: 0,
                };
                self.next_group += 1;
                self.focused = new_group.id;
                self.split_group(group, side, new_group);
            }
        }
        self.normalize();
    }

    /// Set the ratio of the split at `path`, clamped to the ratio
    /// bounds. A path that does not land on a split — the tree changed
    /// under a stale intent — is ignored.
    fn set_ratio(&mut self, path: DockPath, ratio: f32) {
        // A sentinel-less byte is a corrupt address, not the root —
        // ignore it like any other stale path.
        if path.is_corrupt() {
            return;
        }
        let mut idx = Self::ROOT;
        for second in path.directions() {
            let DockNode::Split(s) = self.node(idx) else {
                return;
            };
            idx = if second { s.second } else { s.first };
        }
        if let DockNode::Split(s) = &mut self.nodes[idx.usize()] {
            s.ratio = ratio.clamp(RATIO_MIN, RATIO_MAX);
        }
    }

    /// Drop every tab failing `keep`, collapsing groups that empty.
    pub fn retain_tabs(&mut self, mut keep: impl FnMut(T) -> bool) {
        for node in &mut self.nodes {
            if let DockNode::Group(g) = node {
                g.tabs.retain(|t| keep(*t));
                g.clamp_active();
            }
        }
        self.normalize();
    }

    /// Whether `group`'s pane may still split — the nesting cap. Lets a
    /// drag skip offering edge zones the model would refuse anyway.
    pub fn can_split(&self, group: TabGroupId) -> bool {
        self.group_depth(group).is_some_and(|d| d < self.max_depth)
    }

    /// Number of split ancestors above `id`'s group — what the cap caps.
    fn group_depth(&self, id: TabGroupId) -> Option<u32> {
        fn walk<T>(nodes: &[DockNode<T>], idx: NodeIdx, id: TabGroupId, depth: u32) -> Option<u32> {
            match &nodes[idx.usize()] {
                DockNode::Group(g) => (g.id == id).then_some(depth),
                DockNode::Split(s) => walk(nodes, s.first, id, depth + 1)
                    .or_else(|| walk(nodes, s.second, id, depth + 1)),
            }
        }
        walk(&self.nodes, Self::ROOT, id, 0)
    }

    /// Replace the `target` group's node with a split of it and
    /// `new_group` on `side`. The two children are parked at the
    /// vector's end; the caller's `normalize` re-packs to pre-order.
    fn split_group(&mut self, target: TabGroupId, side: SplitSide, new_group: TabGroup<T>) {
        let Some(slot) = self
            .nodes
            .iter()
            .position(|n| matches!(n, DockNode::Group(g) if g.id == target))
        else {
            return;
        };
        let existing_idx = NodeIdx(self.nodes.len() as u32);
        let fresh_idx = NodeIdx(self.nodes.len() as u32 + 1);
        let (first, second) = if side.new_pane_first() {
            (fresh_idx, existing_idx)
        } else {
            (existing_idx, fresh_idx)
        };
        let existing = std::mem::replace(
            &mut self.nodes[slot],
            DockNode::Split(DockSplit {
                dir: side.dir(),
                ratio: 0.5,
                first,
                second,
            }),
        );
        self.nodes.push(existing);
        self.nodes.push(DockNode::Group(new_group));
    }

    /// Re-pack `nodes` into canonical pre-order from the root, dropping
    /// empty groups and dissolving splits left with one live child, then
    /// re-point a dangling focus at the primary group. The primary group
    /// always survives — the pinned tab never closes — so the root
    /// cannot die.
    fn normalize(&mut self) {
        // Liveness per slot, bottom-up: a group lives while it has tabs,
        // a split while either child does.
        fn alive<T>(nodes: &[DockNode<T>], idx: NodeIdx) -> bool {
            match &nodes[idx.usize()] {
                DockNode::Group(g) => !g.tabs.is_empty(),
                DockNode::Split(s) => alive(nodes, s.first) || alive(nodes, s.second),
            }
        }
        // Pre-order copy of the live tree; a split with one live child
        // dissolves into that child in place.
        fn copy<T: Clone>(
            src: &[DockNode<T>],
            idx: NodeIdx,
            out: &mut Vec<DockNode<T>>,
        ) -> NodeIdx {
            match &src[idx.usize()] {
                DockNode::Group(g) => {
                    out.push(DockNode::Group(g.clone()));
                    NodeIdx(out.len() as u32 - 1)
                }
                DockNode::Split(s) => match (alive(src, s.first), alive(src, s.second)) {
                    (true, true) => {
                        // Reserve the parent's pre-order slot; the
                        // children land right after it.
                        let slot = out.len();
                        out.push(DockNode::Split(*s));
                        let first = copy(src, s.first, out);
                        let second = copy(src, s.second, out);
                        out[slot] = DockNode::Split(DockSplit {
                            first,
                            second,
                            ..*s
                        });
                        NodeIdx(slot as u32)
                    }
                    (true, false) => copy(src, s.first, out),
                    (false, true) => copy(src, s.second, out),
                    (false, false) => unreachable!("a dead subtree is dissolved by its parent"),
                },
            }
        }
        assert!(
            alive(&self.nodes, Self::ROOT),
            "the pinned tab keeps the tree non-empty"
        );
        let mut out = Vec::with_capacity(self.nodes.len());
        copy(&self.nodes, Self::ROOT, &mut out);
        self.nodes = out;
        if self.group(self.focused).is_none() {
            self.focused = self.primary().id;
        }
    }

    /// Structural validation, in every build — see the module doc for
    /// the invariant list.
    ///
    /// A deserialized tree is untrusted input, so a violation is a
    /// returned error rather than a panic, and every index is
    /// bounds-checked before the slot it names is read.
    pub fn validate(&self) -> Result<(), DockError<T>> {
        // Canonical pre-order: walking the tree must visit exactly the
        // slots `0..len` in order, which covers reachability, dead slots
        // and acyclicity in one sweep.
        fn walk<T>(
            nodes: &[DockNode<T>],
            idx: NodeIdx,
            depth: u32,
            cap: u32,
            expect: &mut u32,
        ) -> Result<(), DockError<T>> {
            if idx.0 != *expect {
                return Err(DockError::NonCanonical);
            }
            if idx.usize() >= nodes.len() {
                return Err(DockError::NodeOutOfRange { index: idx.0 });
            }
            *expect += 1;
            if let DockNode::Split(s) = &nodes[idx.usize()] {
                if depth >= cap {
                    return Err(DockError::SplitNesting);
                }
                if !(RATIO_MIN..=RATIO_MAX).contains(&s.ratio) {
                    return Err(DockError::SplitRatio { ratio: s.ratio });
                }
                walk(nodes, s.first, depth + 1, cap, expect)?;
                walk(nodes, s.second, depth + 1, cap, expect)?;
            }
            Ok(())
        }
        let mut expect = 0;
        walk(&self.nodes, Self::ROOT, 0, self.max_depth, &mut expect)?;
        if expect as usize != self.nodes.len() {
            return Err(DockError::UnreachableSlots);
        }

        // Resolved by hand rather than through `primary`, which panics —
        // a corrupt tree may hold no pinned tab at all.
        self.groups()
            .find(|g| g.tabs.contains(&self.pinned))
            .ok_or(DockError::MissingPinnedTab)?;
        let mut seen = Vec::new();
        let mut seen_groups = Vec::new();
        for g in self.groups() {
            // Group ids address every op, and the lookups take the first
            // match, so a duplicate silently retargets ops.
            if seen_groups.contains(&g.id) {
                return Err(DockError::DuplicateGroup { group: g.id });
            }
            seen_groups.push(g.id);
            if g.tabs.is_empty() {
                return Err(DockError::EmptyGroup { group: g.id });
            }
            if g.active >= g.tabs.len() {
                return Err(DockError::ActiveTabOutOfRange { group: g.id });
            }
            for tab in &g.tabs {
                if seen.contains(tab) {
                    return Err(DockError::DuplicateTab { tab: *tab });
                }
                seen.push(*tab);
            }
        }
        if self.group(self.focused).is_none() {
            return Err(DockError::MissingFocusedGroup {
                group: self.focused,
            });
        }
        Ok(())
    }

    /// The id everything else this dock records derives from.
    pub fn dock_id(&self) -> WidgetId {
        WidgetId::from_hash(("palantir.dock", self.seed))
    }

    /// A group's pane container — strip row and content together. The
    /// rect the drop classification keys off.
    pub fn pane_id(&self, group: TabGroupId) -> WidgetId {
        self.dock_id().with(("pane", group))
    }

    /// A group's *content* area — the space below the strip that the
    /// active tab's view fills.
    ///
    /// Keyed by the group rather than by the tab it happens to be
    /// showing, which is the whole point: switching tabs leaves this
    /// widget in place, so a view can be handed its arranged size on the
    /// very pass it first records.
    pub fn content_id(&self, group: TabGroupId) -> WidgetId {
        self.dock_id().with(("content", group))
    }

    /// A group's tab strip.
    pub fn strip_id(&self, group: TabGroupId) -> WidgetId {
        self.dock_id().with(("strip", group))
    }

    /// The splitter at a tree path.
    pub fn splitter_id(&self, path: DockPath) -> WidgetId {
        self.dock_id().with(("splitter", path))
    }

    pub(crate) fn drag_id(&self) -> WidgetId {
        self.dock_id().with("drag")
    }

    /// The chip key a tab is drawn under — the one derivation, so the
    /// strip and a caller polling last frame's responses ask the same
    /// question.
    pub fn tab_key(tab: T) -> u64 {
        WidgetId::from_hash(tab).0
    }

    /// Navigation-phase scan: focus follows a press into a pane, then
    /// one pass over every strip's last-frame chip responses — close
    /// clicks (which win over activation), activation clicks, and the
    /// drag arming — then the in-flight drag's lifecycle.
    ///
    /// **Run this before the record, and apply what it emits.** Palantir
    /// cannot see this frame's layout during a record, so a widget that
    /// learned of a tab click mid-record would draw the pane the click
    /// replaced. Scanning a phase earlier settles the new arrangement
    /// first, so a switch — or a committed drop — draws on the frame it
    /// lands rather than the one after.
    pub fn scan(&self, ui: &mut Ui, ops: &mut Vec<DockOp<T>>) {
        // Ahead of the chip pass: a read-only focus query that only ever
        // moves `focused`, so it composes with an activation from the
        // same scan rather than racing it.
        if let Some(group) = self
            .groups()
            .find(|g| g.id != self.focused && ui.focus_within(self.pane_id(g.id)))
        {
            ops.push(DockOp::FocusPane { group: group.id });
        }
        let mut dragged = self.drag(ui);
        for group in self.groups() {
            let strip = self.strip_id(group.id);
            for &tab in &group.tabs {
                let key = Self::tab_key(tab);
                if ui
                    .response_for(TabStrip::close_id(strip, key))
                    .left
                    .clicked()
                {
                    ops.push(DockOp::CloseTab { tab });
                    continue;
                }
                let chip = ui.response_for(TabStrip::chip_id(strip, key));
                if chip.left.clicked() {
                    ops.push(DockOp::ActivateTab { tab });
                }
                if dragged.is_none() && chip.left.drag.started() {
                    dragged = Some(tab);
                    self.set_drag(ui, Some(tab));
                }
            }
        }
        let Some(tab) = dragged else {
            return;
        };
        let Some(address) = self.find_tab(tab) else {
            self.set_drag(ui, None);
            return;
        };
        if ui.escape_pressed() {
            self.set_drag(ui, None);
            return;
        }
        // The release edge fires on the chip that caught the press.
        let chip = TabStrip::chip_id(self.strip_id(address.group), Self::tab_key(tab));
        if ui.response_for(chip).left.drag.stopped() {
            if let Some(target) = self.drop_target(ui) {
                ops.push(DockOp::MoveTab {
                    tab,
                    to: target.drop,
                });
            }
            self.set_drag(ui, None);
        }
    }

    /// The tab a pointer is currently carrying, if any.
    pub(crate) fn drag(&self, ui: &Ui) -> Option<T> {
        ui.try_state::<TabDrag<T>>(self.drag_id())
            .and_then(|d| d.tab)
    }

    fn set_drag(&self, ui: &mut Ui, tab: Option<T>) {
        ui.state_mut::<TabDrag<T>>(self.drag_id()).tab = tab;
    }

    /// The drop the pointer currently indicates: the pane whose rect
    /// contains it, classified into a zone.
    ///
    /// Panes tile the dock without overlapping, so plain containment
    /// against last frame's rects is exact. Deliberately *not* a hover
    /// test: the hover resolves only to sensed widgets, and a pane's
    /// content can be entirely inert — the pointer over it hovers
    /// nothing, and the drop would go dark. `None` over a divider, the
    /// chrome around the dock, or off-window; a release there cancels.
    pub(crate) fn drop_target(&self, ui: &mut Ui) -> Option<DropTarget> {
        let p = ui.pointer_pos()?;
        let (edge_fraction, caret_width) = {
            let dock = &ui.theme().dock;
            (dock.edge_fraction, dock.caret_width)
        };
        let (group, pane) = self.groups().find_map(|g| {
            let rect = ui.response_for(self.pane_id(g.id)).rect?;
            rect.contains(p).then_some((g, rect))
        })?;
        let strip_id = self.strip_id(group.id);
        let strip = ui.response_for(strip_id).rect?;
        let can_split = self.can_split(group.id);
        let allowed = self.allowed_splits;
        let dock_id = self.dock_id();
        ui.with_state::<ChipRects, _>(dock_id, |ui, buf| {
            buf.rects.clear();
            // An upper bound, not a count — a tab that recorded no rect
            // drops out — so `reserve`, and a no-op from the drag's
            // second frame on.
            buf.rects.reserve(group.tabs.len());
            buf.rects.extend(group.tabs.iter().filter_map(|&tab| {
                ui.response_for(TabStrip::chip_id(strip_id, Self::tab_key(tab)))
                    .rect
            }));
            Some(
                PaneGeometry {
                    group: group.id,
                    pane,
                    strip,
                    chips: &buf.rects,
                    can_split,
                    allowed,
                    edge_fraction,
                    caret_width,
                }
                .classify(p),
            )
        })
    }

    /// The arranged size of a group's content area, `None` before its
    /// first layout — the one frame in a group's life where a view has
    /// to size itself.
    pub fn content_size(&self, ui: &Ui, group: TabGroupId) -> Option<Vec2> {
        let size = ui.response_for(self.content_id(group)).layout_rect?.size;
        (size.w > 0.0 && size.h > 0.0).then(|| Vec2::new(size.w, size.h))
    }
}

/// The chip rects one drop classification reads.
///
/// Kept on the dock's own state row rather than rebuilt per frame: a
/// held drag asks for them on every pointer move.
#[derive(Debug, Default)]
struct ChipRects {
    rects: Vec<Rect>,
}

#[cfg(test)]
mod test_support {
    use crate::widgets::dock::dock_node::DockNode;
    use crate::widgets::dock::dock_state::DockState;
    use crate::widgets::dock::dock_tab::DockTab;
    use crate::widgets::dock::tab_group::TabGroupId;

    impl<T: DockTab> DockState<T> {
        /// Raw node access, so the validation suite can build the
        /// corrupt trees no public op can produce.
        ///
        /// Reached only from this module's own tests, which is why it is
        /// `test_support` rather than `internals`.
        pub(crate) fn nodes_mut(&mut self) -> &mut Vec<DockNode<T>> {
            &mut self.nodes
        }

        /// Point `focused` at a group without checking it exists — the
        /// dangling-focus corruption.
        pub(crate) fn set_focused_unchecked(&mut self, group: TabGroupId) {
            self.focused = group;
        }

        /// A group id no tree has minted.
        pub(crate) fn absent_group(&self) -> TabGroupId {
            TabGroupId(self.next_group + 1000)
        }
    }
}
