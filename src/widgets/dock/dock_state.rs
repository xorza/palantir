//! The dock's persisted arrangement: a binary split tree whose leaves are tab
//! groups, plus the six operations that mutate it.
//!
//! **Flat storage.** One `Vec<DockNode<T>>` with [`NodeIndex`] children, kept
//! canonical (pre-order from slot 0, no dead slots) by re-packing after every
//! structural operation, so `Vec` equality is structural equality (an undo layer
//! can diff snapshots).
//!
//! Invariants, checked on deserialize so a bad saved layout is an error:
//! - canonical pre-order, fully reachable from slot 0;
//! - some group holds the pinned tab;
//! - no group is empty, no tab appears twice, group ids are unique, each `active`
//!   is in range, `focused` names a live group, every ratio is inside the clamp;
//! - the group-id counter can still mint an id no group holds.

use std::hash::Hash;

use serde::{Deserialize, Serialize};

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::math::domain;
use crate::widgets::dock::allowed_splits::AllowedSplits;
use crate::widgets::dock::dock_node::{DockNode, DockSplit, NodeIndex};
use crate::widgets::dock::dock_operation::{DockDrop, DockOperation};
use crate::widgets::dock::dock_path::DockPath;
use crate::widgets::dock::dock_tab::DockTab;
use crate::widgets::dock::error::DockError;
use crate::widgets::dock::split_side::SplitSide;
use crate::widgets::dock::tab_group::{TabGroup, TabGroupId};
use std::mem;

/// Split-ratio clamp: neither pane drops below a tenth of the split.
const RATIO_MIN: f32 = 0.1;
const RATIO_MAX: f32 = 0.9;

/// Most nested splits on any root-to-leaf chain (up to 16 panes).
const DEFAULT_MAX_DEPTH: u32 = 4;

/// A tab's position: which group holds it, and where in that group's strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabAddress {
    /// The group holding the tab.
    pub group: TabGroupId,
    /// Its slot in that group's strip.
    pub index: usize,
}

/// The whole pane arrangement: the flat split tree, the focused group, and the
/// policy the operations enforce.
///
/// The application owns one per tab domain and persists it. The widget never
/// mutates it: [`DockView`](crate::DockView) emits [`DockOperation`]s and
/// [`Self::apply`] is the one place a mutation happens, so an app can route them
/// through its own queue and keep them out of undo.
///
/// ```
/// # use palantir::{DockOperation, DockState};
/// # #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// # enum Tab { Main, Console }
/// let mut dock = DockState::new("app.dock", Tab::Main);
/// dock.apply(DockOperation::OpenTab { tab: Tab::Console });
/// assert_eq!(dock.groups().count(), 1);
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "RawDockState<T>",
    bound(deserialize = "T: DockTab + Deserialize<'de>")
)]
#[must_use]
pub struct DockState<T> {
    nodes: Vec<DockNode<T>>,
    focused: TabGroupId,
    next_group: u64,
    pinned: T,
    seed: u64,
    /// Policy, not document: not serialised; apply a changed cap after loading.
    #[serde(skip_serializing)]
    max_depth: u32,
    #[serde(skip_serializing)]
    allowed_splits: AllowedSplits,
}

/// The document half of a [`DockState`] as a file holds it, before validation.
#[derive(Debug, Deserialize)]
struct RawDockState<T> {
    nodes: Vec<DockNode<T>>,
    focused: TabGroupId,
    next_group: u64,
    pinned: T,
    seed: u64,
}

impl<T: DockTab> TryFrom<RawDockState<T>> for DockState<T> {
    type Error = DockError<T>;

    /// The file's tree under the default policy, if it holds every module-doc invariant.
    fn try_from(raw: RawDockState<T>) -> Result<Self, Self::Error> {
        let RawDockState {
            nodes,
            focused,
            next_group,
            pinned,
            seed,
        } = raw;
        let state = Self {
            nodes,
            focused,
            next_group,
            pinned,
            seed,
            max_depth: DEFAULT_MAX_DEPTH,
            allowed_splits: AllowedSplits::default(),
        };
        state.validate()?;
        Ok(state)
    }
}

impl<T: DockTab> DockState<T> {
    /// The root node's index: always slot 0.
    pub const ROOT: NodeIndex = NodeIndex(0);

    /// Smallest share a split gives either pane.
    pub const RATIO_MIN: f32 = RATIO_MIN;
    /// Largest share a split gives the first pane; mirror of [`Self::RATIO_MIN`].
    pub const RATIO_MAX: f32 = RATIO_MAX;

    /// A dock holding one pane, showing `pinned`. `seed` scopes every widget id this
    /// dock derives; use one stable string per tab domain. `pinned` refuses to close,
    /// so the root always survives and the arrangement is never empty.
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

    /// Which split directions a drag offers. Default [`AllowedSplits::All`].
    pub const fn with_allowed_splits(mut self, allowed: AllowedSplits) -> Self {
        self.allowed_splits = allowed;
        self
    }

    /// The tab that refuses to close.
    pub const fn pinned(&self) -> T {
        self.pinned
    }

    /// The seed this dock's widget ids derive from.
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// The split directions a drop may create; see [`Self::with_allowed_splits`].
    pub const fn allowed_splits(&self) -> AllowedSplits {
        self.allowed_splits
    }

    /// The group keyboard shortcuts and newly opened tabs go to.
    pub const fn focused(&self) -> TabGroupId {
        self.focused
    }

    /// The node at `index`; the record walk follows [`DockSplit`]'s child indices.
    pub fn node(&self, index: NodeIndex) -> &DockNode<T> {
        &self.nodes[index.usize()]
    }

    /// The leaf groups in left-to-right, top-to-bottom pane order (vector order).
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

    /// Execute one [`DockOperation`].
    pub fn apply(&mut self, operation: DockOperation<T>) {
        match operation {
            DockOperation::ActivateTab { tab } => self.activate(tab),
            DockOperation::OpenTab { tab } => self.open_tab(tab),
            DockOperation::CloseTab { tab } => self.close_tab(tab),
            DockOperation::MoveTab { tab, to } => self.move_tab(tab, to),
            DockOperation::SetRatio { split, ratio } => self.set_ratio(split, ratio),
            DockOperation::FocusPane { group } => self.focus(group),
        }
    }

    /// Move focus onto `group`. A gone group no-ops, like any stale address; a dead id
    /// in `focused` would fail [`Self::validate`] at the next load.
    fn focus(&mut self, group: TabGroupId) {
        if self.group(group).is_some() {
            self.focused = group;
        }
    }

    /// Add `tab` to the focused group unless already open somewhere, then activate it.
    fn open_tab(&mut self, tab: T) {
        self.find_or_insert(tab, self.focused);
        self.activate(tab);
    }

    /// Make `tab` visible in whichever group holds it and focus that group. A closed
    /// tab no-ops.
    fn activate(&mut self, tab: T) {
        let Some(TabAddress { group, index }) = self.find_tab(tab) else {
            return;
        };
        self.group_mut(group)
            .expect("find_tab resolved a live group")
            .active = index;
        self.focused = group;
    }

    /// Append `tab` to `group`'s strip unless already open: the tree half of
    /// [`DockOperation::OpenTab`]. Callers hold a live group, so a dead id is a logic
    /// error.
    pub fn find_or_insert(&mut self, tab: T, group: TabGroupId) {
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
        let Some(TabAddress { group, index }) = self.find_tab(tab) else {
            return;
        };
        self.group_mut(group)
            .expect("find_tab resolved a live group")
            .remove_tab(index);
        self.normalize();
    }

    /// Move `tab` to `drop`, collapsing whatever its departure empties. An `Into`
    /// index addresses the target strip as the caller saw it, before the move. The
    /// destination takes the tab as active and gains focus. Degenerate moves (splitting
    /// a lone tab off its own group) leave the tree unchanged.
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
        // Depth cap, checked before any mutation so a refused split can't lose the tab.
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
                // `index` addresses the strip as the caller saw it; a rightward move in one group
                // compensates for its own removal.
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

    /// Set the ratio of the split at `path`, clamped, centred when not finite. A path
    /// that no longer lands on a split is ignored.
    fn set_ratio(&mut self, path: DockPath, ratio: f32) {
        // A sentinel-less byte is a corrupt address; ignore it like a stale path.
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
            s.ratio = domain::fraction_or(ratio, 0.5).clamp(RATIO_MIN, RATIO_MAX);
        }
    }

    /// Drop every tab failing `keep`, collapsing emptied groups. The pinned tab is
    /// never offered: a filter taking it would yield a state that fails to load and
    /// that [`Self::primary`] panics on.
    pub fn retain_tabs(&mut self, mut keep: impl FnMut(T) -> bool) {
        let pinned = self.pinned;
        for node in &mut self.nodes {
            if let DockNode::Group(g) = node {
                g.retain(|tab| *tab == pinned || keep(*tab));
            }
        }
        self.normalize();
    }

    /// Whether `group`'s pane may still split (the nesting cap).
    pub fn can_split(&self, group: TabGroupId) -> bool {
        self.group_depth(group).is_some_and(|d| d < self.max_depth)
    }

    /// Number of split ancestors above `id`'s group.
    fn group_depth(&self, id: TabGroupId) -> Option<u32> {
        fn walk<T>(
            nodes: &[DockNode<T>],
            idx: NodeIndex,
            id: TabGroupId,
            depth: u32,
        ) -> Option<u32> {
            match &nodes[idx.usize()] {
                DockNode::Group(g) => (g.id == id).then_some(depth),
                DockNode::Split(s) => walk(nodes, s.first, id, depth + 1)
                    .or_else(|| walk(nodes, s.second, id, depth + 1)),
            }
        }
        walk(&self.nodes, Self::ROOT, id, 0)
    }

    /// Replace the `target` group's node with a split of it and `new_group` on `side`.
    /// Children are parked at the vector's end; the caller's `normalize` re-packs.
    fn split_group(&mut self, target: TabGroupId, side: SplitSide, new_group: TabGroup<T>) {
        let Some(slot) = self
            .nodes
            .iter()
            .position(|n| matches!(n, DockNode::Group(g) if g.id == target))
        else {
            return;
        };
        let existing_idx = NodeIndex(self.nodes.len() as u32);
        let fresh_idx = NodeIndex(self.nodes.len() as u32 + 1);
        let (first, second) = if side.new_pane_first() {
            (fresh_idx, existing_idx)
        } else {
            (existing_idx, fresh_idx)
        };
        let existing = mem::replace(
            &mut self.nodes[slot],
            DockNode::Split(DockSplit {
                direction: side.direction(),
                ratio: 0.5,
                first,
                second,
            }),
        );
        self.nodes.push(existing);
        self.nodes.push(DockNode::Group(new_group));
    }

    /// Re-pack `nodes` into canonical pre-order, dropping empty groups and dissolving
    /// splits with one live child, then re-point a dangling focus at the primary group.
    fn normalize(&mut self) {
        // Liveness bottom-up.
        fn alive<T>(nodes: &[DockNode<T>], idx: NodeIndex) -> bool {
            match &nodes[idx.usize()] {
                DockNode::Group(g) => !g.tabs.is_empty(),
                DockNode::Split(s) => alive(nodes, s.first) || alive(nodes, s.second),
            }
        }
        // Pre-order copy; a split with one live child dissolves into it.
        fn copy<T: Clone>(
            src: &[DockNode<T>],
            idx: NodeIndex,
            out: &mut Vec<DockNode<T>>,
        ) -> NodeIndex {
            match &src[idx.usize()] {
                DockNode::Group(g) => {
                    out.push(DockNode::Group(g.clone()));
                    NodeIndex(out.len() as u32 - 1)
                }
                DockNode::Split(s) => match (alive(src, s.first), alive(src, s.second)) {
                    (true, true) => {
                        // Reserve the parent's slot; the children land right after.
                        let slot = out.len();
                        out.push(DockNode::Split(*s));
                        let first = copy(src, s.first, out);
                        let second = copy(src, s.second, out);
                        out[slot] = DockNode::Split(DockSplit {
                            first,
                            second,
                            ..*s
                        });
                        NodeIndex(slot as u32)
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

    /// Structural validation in every build. A deserialized tree is untrusted, so a
    /// violation is a returned error and indices are bounds-checked first.
    pub(crate) fn validate(&self) -> Result<(), DockError<T>> {
        // Walking must visit exactly slots `0..len` in order: reachability, dead slots
        // and acyclicity in one sweep.
        fn walk<T>(
            nodes: &[DockNode<T>],
            idx: NodeIndex,
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
        // Structure, not policy: a loaded layout is checked against the depth a
        // `DockPath` addresses; `max_depth` governs later splits only.
        walk(&self.nodes, Self::ROOT, 0, DockPath::CAPACITY, &mut expect)?;
        if expect as usize != self.nodes.len() {
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
        if self.next_group == u64::MAX || seen_groups.iter().any(|g| g.0 >= self.next_group) {
            return Err(DockError::GroupAllocator {
                next_group: self.next_group,
            });
        }
        if self.group(self.focused).is_none() {
            return Err(DockError::MissingFocusedGroup {
                group: self.focused,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod internals {
    use crate::widgets::dock::dock_node::DockNode;
    use crate::widgets::dock::dock_state::DockState;
    use crate::widgets::dock::dock_tab::DockTab;
    use crate::widgets::dock::tab_group::TabGroupId;

    impl<T: DockTab> DockState<T> {
        /// Raw node access for the validation suite to build corrupt trees.
        pub(crate) fn nodes_mut(&mut self) -> &mut Vec<DockNode<T>> {
            &mut self.nodes
        }

        /// Point `focused` at a group without checking it exists.
        pub(crate) fn set_focused_unchecked(&mut self, group: TabGroupId) {
            self.focused = group;
        }

        pub(crate) fn absent_group(&self) -> TabGroupId {
            TabGroupId(self.next_group + 1000)
        }

        /// Park the id counter where no operation sequence could.
        pub(crate) fn set_next_group_unchecked(&mut self, next: u64) {
            self.next_group = next;
        }
    }
}
