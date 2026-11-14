//! The dock's persisted arrangement: a binary split tree whose leaves are tab
//! groups, plus the six operations that mutate it.
//!
//! **Flat storage.** One `Vec<DockNode<T>>` with [`NodeIdx`] children, kept
//! canonical (pre-order from slot 0, no dead slots) by re-packing after every
//! structural operation, so `Vec` equality is structural equality (an undo layer
//! can diff snapshots).
//!
//! Invariants, checked by [`DockState::load`] so a bad saved layout is an error:
//! - canonical pre-order, fully reachable from slot 0;
//! - some group holds the pinned tab;
//! - no group is empty, no tab appears twice, group ids are unique, each `active`
//!   is in range, `focused` names a live group, every ratio is inside the clamp;
//! - the group-id counter can still mint an id no group holds.

use crate::primitives::math::domain;
use crate::widgets::dock::dock_layout::DockLayout;
use crate::widgets::dock::dock_node::{DockNode, DockSplit, NodeIdx};
use crate::widgets::dock::dock_operation::{DockDrop, DockOperation};
use crate::widgets::dock::dock_path::DockPath;
use crate::widgets::dock::dock_tab::DockTab;
use crate::widgets::dock::error::DockError;
use crate::widgets::dock::split_side::SplitSide;
use crate::widgets::dock::tab_group::{TabGroup, TabGroupId};
use crate::widgets::splitter::split_half::SplitHalf;
use crate::widgets::tabs::reindex;
use std::mem;

/// Split-ratio clamp: neither pane drops below a tenth of the split.
pub(crate) const RATIO_MIN: f32 = 0.1;
/// Derived, so the clamp stays the symmetric one a divider drag floors at (`Splitter::min_ratio`).
pub(crate) const RATIO_MAX: f32 = 1.0 - RATIO_MIN;

/// Most nested splits on any root-to-leaf chain (up to 16 panes).
const DEFAULT_MAX_DEPTH: u32 = 4;

/// A tab's position: which group holds it, and where in that group's strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabAddress {
    /// The group holding the tab.
    pub group: TabGroupId,
    /// Its slot in that group's strip.
    pub idx: usize,
}

/// The whole pane arrangement: the flat split tree, the focused group, the pinned tab and the
/// nesting cap the operations enforce.
///
/// The application owns one per tab domain and persists its [`Self::layout`]. The widget never
/// mutates it: [`DockView`](crate::DockView) emits [`DockOperation`]s and [`Self::apply`] is
/// where they land, so an app can route them through its own queue and keep them out of undo.
/// The one edit outside it is [`Self::retain_tabs`], a filter no `Copy` operation can carry.
///
/// ```
/// # use palantir::{DockOperation, DockState};
/// # #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// # enum Tab { Main, Console }
/// let mut dock = DockState::new(Tab::Main);
/// dock.apply(DockOperation::OpenTab { tab: Tab::Console });
/// assert_eq!(dock.groups().count(), 1);
/// ```
#[derive(Clone, Debug, PartialEq)]
#[must_use]
pub struct DockState<T> {
    layout: DockLayout<T>,
    pinned: T,
    max_depth: u32,
}

impl<T: DockTab> DockState<T> {
    /// The root node's index: always slot 0.
    pub(crate) const ROOT: NodeIdx = NodeIdx(0);

    /// A dock holding one pane, showing `pinned`. `pinned` refuses to close, so the root always
    /// survives and the arrangement is never empty.
    pub fn new(pinned: T) -> Self {
        let primary = TabGroup {
            id: TabGroupId(0),
            tabs: vec![pinned],
            active: 0,
        };
        Self {
            layout: DockLayout {
                focused: primary.id,
                nodes: vec![DockNode::Group(primary)],
                next_group: 1,
            },
            pinned,
            max_depth: DEFAULT_MAX_DEPTH,
        }
    }

    /// The dock `layout` arranges, with `pinned` as the tab that refuses to close, if the layout
    /// holds every invariant the module doc lists.
    ///
    /// # Errors
    ///
    /// The [`DockError`] naming the first invariant `layout` breaks, as a file a hand or another
    /// version edited can.
    pub fn load(layout: DockLayout<T>, pinned: T) -> Result<Self, DockError<T>> {
        let state = Self {
            layout,
            pinned,
            max_depth: DEFAULT_MAX_DEPTH,
        };
        state.validate()?;
        Ok(state)
    }

    /// What this dock persists; serialize it to save, and [`Self::load`] it back.
    pub const fn layout(&self) -> &DockLayout<T> {
        &self.layout
    }

    /// Cap the split nesting on any root-to-leaf chain. Default 4 (up to 16 panes). On
    /// the state because [`Self::apply`] refuses a deeper split. It governs new splits
    /// only; apply your cap after loading.
    ///
    /// # Panics
    ///
    /// Panics if `depth` exceeds what a [`DockPath`] can address.
    pub fn with_max_depth(mut self, depth: u32) -> Self {
        assert!(
            depth <= DockPath::CAPACITY,
            "max_depth {depth} exceeds the {} levels a DockPath addresses",
            DockPath::CAPACITY,
        );
        self.max_depth = depth;
        self
    }

    /// The tab that refuses to close.
    pub const fn pinned(&self) -> T {
        self.pinned
    }

    /// The group keyboard shortcuts and newly opened tabs go to.
    pub const fn focused(&self) -> TabGroupId {
        self.layout.focused
    }

    /// The node at `idx`; the record walk follows [`DockSplit`]'s child indices.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is past the last node, as one read before a structural operation can be.
    pub(crate) fn node(&self, idx: NodeIdx) -> &DockNode<T> {
        &self.layout.nodes[idx.usize()]
    }

    /// The leaf groups in left-to-right, top-to-bottom pane order (vector order).
    pub fn groups(&self) -> impl Iterator<Item = &TabGroup<T>> {
        self.layout.nodes.iter().filter_map(|n| match n {
            DockNode::Group(g) => Some(g),
            DockNode::Split(_) => None,
        })
    }

    /// Every open tab across every group, in [`Self::groups`] order.
    pub fn all_tabs(&self) -> impl Iterator<Item = T> + '_ {
        self.groups().flat_map(|g| g.tabs.iter().copied())
    }

    /// What each pane is showing: one tab per group, in [`Self::groups`] order.
    pub fn active_tabs(&self) -> impl Iterator<Item = T> + '_ {
        self.groups().map(TabGroup::active_tab)
    }

    /// The group holding the pinned tab, the one pane that always exists.
    pub fn primary(&self) -> &TabGroup<T> {
        self.groups()
            .find(|g| g.tabs.contains(&self.pinned))
            .expect("a group holds the pinned tab")
    }

    /// Where `tab` currently sits, or `None` when no group holds it.
    pub fn find_tab(&self, tab: T) -> Option<TabAddress> {
        self.groups().find_map(|g| {
            g.tabs
                .iter()
                .position(|t| *t == tab)
                .map(|idx| TabAddress { group: g.id, idx })
        })
    }

    /// One group by id, or `None` once it has collapsed.
    pub fn group(&self, id: TabGroupId) -> Option<&TabGroup<T>> {
        self.groups().find(|g| g.id == id)
    }

    fn group_mut(&mut self, id: TabGroupId) -> Option<&mut TabGroup<T>> {
        self.layout.nodes.iter_mut().find_map(|n| match n {
            DockNode::Group(g) if g.id == id => Some(g),
            _ => None,
        })
    }

    /// Execute one [`DockOperation`].
    pub fn apply(&mut self, operation: DockOperation<T>) {
        match operation {
            DockOperation::ActivateTab { tab } => self.activate(tab),
            DockOperation::OpenTab { tab } => self.open_tab(tab),
            DockOperation::CloseTab { tab } => self.close_tab(tab),
            DockOperation::MoveTab { tab, to } => self.move_tab(tab, to),
            DockOperation::SetRatio { split, ratio } => self.set_ratio(split, ratio),
            DockOperation::FocusGroup { group } => self.focus(group),
        }
    }

    /// Move focus onto `group`. A gone group no-ops, like any stale address; a dead id
    /// in `focused` would fail [`Self::validate`] at the next load.
    fn focus(&mut self, group: TabGroupId) {
        if self.group(group).is_some() {
            self.layout.focused = group;
        }
    }

    /// Add `tab` to the focused group unless already open somewhere, then activate it.
    fn open_tab(&mut self, tab: T) {
        self.find_or_insert(tab, self.layout.focused);
        self.activate(tab);
    }

    /// Make `tab` visible in whichever group holds it and focus that group. A closed
    /// tab no-ops.
    fn activate(&mut self, tab: T) {
        let Some(TabAddress { group, idx }) = self.find_tab(tab) else {
            return;
        };
        self.group_mut(group)
            .expect("find_tab resolved a live group")
            .active = idx;
        self.layout.focused = group;
    }

    /// Append `tab` to `group`'s strip unless already open: the tree half of
    /// [`DockOperation::OpenTab`]. Callers hold a live group, so a dead id is a logic
    /// error.
    fn find_or_insert(&mut self, tab: T, group: TabGroupId) {
        if self.find_tab(tab).is_none() {
            self.group_mut(group)
                .expect("insert target group exists")
                .tabs
                .push(tab);
        }
    }

    /// Close `tab` wherever it sits. The pinned tab never closes. An emptied group
    /// collapses; a vanished focus falls back to the primary group.
    fn close_tab(&mut self, tab: T) {
        if tab == self.pinned {
            return;
        }
        let Some(TabAddress { group, idx }) = self.find_tab(tab) else {
            return;
        };
        self.group_mut(group)
            .expect("find_tab resolved a live group")
            .remove_tab(idx);
        self.normalize();
    }

    /// Move `tab` to `drop`, collapsing whatever its departure empties. An `Into`
    /// index addresses the target strip as the caller saw it, before the move. The
    /// destination takes the tab as active and gains focus. Degenerate moves (splitting
    /// a lone tab off its own group) leave the tree unchanged.
    fn move_tab(&mut self, tab: T, drop: DockDrop) {
        // Checked before any mutation, so a refused split can't lose the tab.
        if !self.accepts_move(tab, drop) {
            return;
        }
        let source = self.find_tab(tab).expect("an accepted tab is held");

        self.group_mut(source.group)
            .expect("source exists")
            .remove_tab(source.idx);

        match drop {
            DockDrop::Join { group, idx } => {
                // `idx` addresses the strip as the caller saw it, before the tab left it.
                let idx = if group == source.group {
                    reindex::landing(source.idx, idx)
                } else {
                    idx
                };
                let g = self.group_mut(group).expect("target exists");
                let idx = idx.min(g.tabs.len());
                g.tabs.insert(idx, tab);
                g.active = idx;
                self.layout.focused = group;
            }
            DockDrop::Split { group, side } => {
                let new_group = TabGroup {
                    id: TabGroupId(self.layout.next_group),
                    tabs: vec![tab],
                    active: 0,
                };
                self.layout.next_group += 1;
                self.layout.focused = new_group.id;
                self.split_group(group, side, new_group);
            }
        }
        self.normalize();
    }

    /// Set the ratio of the split at `path`, clamped, centered when not finite. A path
    /// that no longer lands on a split is ignored.
    fn set_ratio(&mut self, path: DockPath, ratio: f32) {
        // A sentinel-less byte is a corrupt address; ignore it like a stale path.
        if path.is_corrupt() {
            return;
        }
        let mut idx = Self::ROOT;
        for half in path.directions() {
            let DockNode::Split(s) = self.node(idx) else {
                return;
            };
            idx = s.child(half);
        }
        if let DockNode::Split(s) = &mut self.layout.nodes[idx.usize()] {
            s.ratio = domain::fraction_or(ratio, 0.5).clamp(RATIO_MIN, RATIO_MAX);
        }
    }

    /// Drop every tab failing `keep`, collapsing emptied groups. The pinned tab is
    /// never offered: a filter taking it would yield a state that fails to load and
    /// that [`Self::primary`] panics on.
    pub fn retain_tabs(&mut self, mut keep: impl FnMut(T) -> bool) {
        let pinned = self.pinned;
        for node in &mut self.layout.nodes {
            if let DockNode::Group(g) = node {
                g.retain(|tab| *tab == pinned || keep(*tab));
            }
        }
        self.normalize();
    }

    /// Whether moving `tab` to `drop` changes the tree: the tab and the target are held, a lone
    /// tab neither splits off nor moves within its own group, and a split stays under the
    /// [`Self::with_max_depth`] cap. The one rule [`Self::apply`] moves by and a dock's drag
    /// shows its drop by, so no wash promises a move the release refuses.
    pub(crate) fn accepts_move(&self, tab: T, drop: DockDrop) -> bool {
        let Some(source) = self.find_tab(tab) else {
            return false;
        };
        let target = match drop {
            DockDrop::Join { group, .. } | DockDrop::Split { group, .. } => group,
        };
        if self.group(target).is_none() {
            return false;
        }
        let source_len = self.group(source.group).expect("source exists").tabs.len();
        if source.group == target && source_len == 1 {
            return false;
        }
        !matches!(drop, DockDrop::Split { .. }) || self.can_split(target)
    }

    /// Whether a [`DockDrop::Split`] on `group` takes effect: `false` for a group at the
    /// [`Self::with_max_depth`] cap, whose split [`Self::apply`] ignores, and for a group the
    /// state does not hold. A menu item that splits a pane reads it to show disabled.
    pub fn can_split(&self, group: TabGroupId) -> bool {
        self.group_depth(group).is_some_and(|d| d < self.max_depth)
    }

    /// Number of split ancestors above `id`'s group.
    fn group_depth(&self, id: TabGroupId) -> Option<u32> {
        self.depth_below(Self::ROOT, id)
    }

    /// Number of splits between `idx` and `id`'s group, if the group is under `idx`.
    fn depth_below(&self, idx: NodeIdx, id: TabGroupId) -> Option<u32> {
        match self.node(idx) {
            DockNode::Group(g) => (g.id == id).then_some(0),
            DockNode::Split(s) => self
                .depth_below(s.first, id)
                .or_else(|| self.depth_below(s.second, id))
                .map(|depth| depth + 1),
        }
    }

    /// Replace the `target` group's node with a split of it and `new_group` on `side`.
    /// Children are parked at the vector's end; the caller's `normalize` re-packs.
    fn split_group(&mut self, target: TabGroupId, side: SplitSide, new_group: TabGroup<T>) {
        let Some(slot) = self
            .layout
            .nodes
            .iter()
            .position(|n| matches!(n, DockNode::Group(g) if g.id == target))
        else {
            return;
        };
        let existing_idx = NodeIdx::at(self.layout.nodes.len());
        let fresh_idx = NodeIdx::at(self.layout.nodes.len() + 1);
        let (first, second) = match side.half() {
            SplitHalf::First => (fresh_idx, existing_idx),
            SplitHalf::Second => (existing_idx, fresh_idx),
        };
        let existing = mem::replace(
            &mut self.layout.nodes[slot],
            DockNode::Split(DockSplit {
                axis: side.axis(),
                ratio: 0.5,
                first,
                second,
            }),
        );
        self.layout.nodes.push(existing);
        self.layout.nodes.push(DockNode::Group(new_group));
    }

    /// Re-pack `nodes` into canonical pre-order, dropping empty groups and dissolving
    /// splits with one live child, then re-point a dangling focus at the primary group.
    fn normalize(&mut self) {
        assert!(
            self.is_alive(Self::ROOT),
            "the pinned tab keeps the tree non-empty"
        );
        let mut out = Vec::with_capacity(self.layout.nodes.len());
        self.copy_live(Self::ROOT, &mut out);
        self.layout.nodes = out;
        if self.group(self.layout.focused).is_none() {
            self.layout.focused = self.primary().id;
        }
    }

    /// Whether any tab survives under `idx`.
    fn is_alive(&self, idx: NodeIdx) -> bool {
        match self.node(idx) {
            DockNode::Group(g) => !g.tabs.is_empty(),
            DockNode::Split(s) => self.is_alive(s.first) || self.is_alive(s.second),
        }
    }

    /// Copy the live subtree under `idx` onto `out` in pre-order, returning where
    /// it landed; a split with one live child dissolves into it.
    fn copy_live(&self, idx: NodeIdx, out: &mut Vec<DockNode<T>>) -> NodeIdx {
        match self.node(idx) {
            DockNode::Group(g) => {
                out.push(DockNode::Group(g.clone()));
                NodeIdx::at(out.len() - 1)
            }
            DockNode::Split(s) => match (self.is_alive(s.first), self.is_alive(s.second)) {
                (true, true) => {
                    // Reserve the parent's slot; the children land right after.
                    let slot = out.len();
                    out.push(DockNode::Split(*s));
                    let first = self.copy_live(s.first, out);
                    let second = self.copy_live(s.second, out);
                    out[slot] = DockNode::Split(DockSplit {
                        first,
                        second,
                        ..*s
                    });
                    NodeIdx::at(slot)
                }
                (true, false) => self.copy_live(s.first, out),
                (false, true) => self.copy_live(s.second, out),
                (false, false) => unreachable!("a dead subtree is dissolved by its parent"),
            },
        }
    }

    /// Structural validation in every build. A deserialized tree is untrusted, so a
    /// violation is a returned error and indices are bounds-checked first.
    pub(super) fn validate(&self) -> Result<(), DockError<T>> {
        let mut expect = 0;
        self.check_canonical(Self::ROOT, 0, &mut expect)?;
        if expect as usize != self.layout.nodes.len() {
            return Err(DockError::UnreachableSlots);
        }

        // By hand: `primary` panics and a corrupt tree may hold no pinned tab.
        self.groups()
            .find(|g| g.tabs.contains(&self.pinned))
            .ok_or(DockError::MissingPinnedTab)?;
        let mut seen = Vec::new();
        let mut seen_groups = Vec::new();
        for g in self.groups() {
            // Lookups take the first match, so a duplicate id would silently retarget.
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
        // The id counter is document state: check it here, not at the split that spends
        // it. It must name an id no group holds and have room left to count.
        if self.layout.next_group == u64::MAX
            || seen_groups.iter().any(|g| g.0 >= self.layout.next_group)
        {
            return Err(DockError::GroupAllocator {
                next_group: self.layout.next_group,
            });
        }
        if self.group(self.layout.focused).is_none() {
            return Err(DockError::MissingFocusedGroup {
                group: self.layout.focused,
            });
        }
        Ok(())
    }

    /// Walk the subtree at `idx`, which must sit in slot `*expect`, at `depth`
    /// splits below the root. Walking must visit exactly slots `0..len` in order:
    /// reachability, dead slots and acyclicity in one sweep. Indices are
    /// bounds-checked before they are read, since the tree is untrusted.
    ///
    /// Structure, not policy: depth is checked against what a [`DockPath`]
    /// addresses; `max_depth` governs later splits only.
    fn check_canonical(
        &self,
        idx: NodeIdx,
        depth: u32,
        expect: &mut u32,
    ) -> Result<(), DockError<T>> {
        if idx.0 != *expect {
            return Err(DockError::NonCanonical);
        }
        let Some(node) = self.layout.nodes.get(idx.usize()) else {
            return Err(DockError::NodeOutOfRange { idx: idx.0 });
        };
        *expect += 1;
        if let DockNode::Split(s) = node {
            if depth >= DockPath::CAPACITY {
                return Err(DockError::SplitNesting);
            }
            if !(RATIO_MIN..=RATIO_MAX).contains(&s.ratio) {
                return Err(DockError::SplitRatio { ratio: s.ratio });
            }
            self.check_canonical(s.first, depth + 1, expect)?;
            self.check_canonical(s.second, depth + 1, expect)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod internals {
    use crate::widgets::dock::dock_state::DockState;
    use crate::widgets::dock::dock_tab::DockTab;
    use crate::widgets::dock::tab_group::TabGroupId;

    impl<T: DockTab> DockState<T> {
        pub(crate) fn absent_group(&self) -> TabGroupId {
            TabGroupId(self.layout.next_group + 1000)
        }

        /// Append `tab` to a live `group` without activating it or moving focus.
        pub(crate) fn insert_into(&mut self, tab: T, group: TabGroupId) {
            self.find_or_insert(tab, group);
        }
    }
}
