//! The recorded half of the scene: one arena per plane, plus the per-frame
//! identity tracker and plane stack that recording needs.

use crate::common::tracy;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::layout_mode::{GridDefId, ScrollbarsDefId};
use crate::primitives::layout::placement::Placement;
use crate::primitives::layout::track::Track;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::endpoint::Endpoint;
use crate::scene::layer::Layer;
use crate::scene::node::Node;
use crate::scene::node::ident::Ident;
use crate::scene::per_plane::PerPlane;
use crate::scene::plane::Plane;
use crate::scene::scrollbars_def::{ResolvedScrollbarsDef, ScrollbarsDef};
use crate::scene::seen_ids::{CollisionRecord, ResolvedId, SeenIds};
use crate::scene::tree::ChromeInput;
use crate::scene::tree::Tree;
use crate::scene::tree::paint_animations::PaintAnimationEntry;
use crate::scene::tree::recording_scratch::OpenFrame;
use crate::scene::tree::recording_scratch::RecordingScratch;
use crate::shape::Lower;
use crate::shape::paint_animation::PaintAnimation;
use crate::shape::record_store::RecordStore;
use crate::text::family_keys::FamilyKeys;
use std::time::Duration;

/// One arena per [`Plane`]; recording dispatches to `trees[cur_plane]`.
#[derive(Debug, Default)]
pub(crate) struct Forest {
    pub(crate) trees: PerPlane<Tree>,
    /// Variable-sized payloads referenced by shape records; cleared on a record pass, retained across `PaintOnly` frames.
    pub(crate) record_store: RecordStore,
    /// Per-plane recording-only state, kept off `Tree` so downstream passes can't reach it. Cleared by `pre_record`.
    scratch: PerPlane<RecordingScratch>,
    /// Per-frame `WidgetId` tracker (collision detection, auto-id disambiguation). Lives here so every path to `open_node` gets the check.
    pub(crate) ids: SeenIds,
    /// Explicit-id collisions this frame, read by `encoder::emit_collision_overlays`.
    ///
    /// Recorded in every profile for tests, but only painted in a development build.
    pub(crate) collisions: Vec<CollisionRecord>,
    /// Planes of the active side-layer scopes; empty for the `Main` baseline.
    ///
    /// Each pushed plane ranks strictly above the one below it (see [`Self::push_layer`]), so a plane is on the stack at most once and its `pending_placement` is single-occupancy.
    plane_stack: Vec<Plane>,
}

/// What paints behind a node [`Forest::open_node`] opens: its chrome, borrowed so a chromed widget doesn't re-copy it down the record chain, and the focus ring over it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NodeChrome<'a> {
    /// `None` for a chrome-less node.
    pub(crate) chrome: Option<&'a Chrome>,
    /// `Stroke::NONE` unless the node shows keyboard focus.
    pub(crate) ring: Stroke,
}

impl Forest {
    /// A forest whose text records key their families in `families`, the shaper's table.
    pub(crate) fn new(families: FamilyKeys) -> Self {
        Self {
            record_store: RecordStore::new(families),
            ..Self::default()
        }
    }

    /// Active plane for the next `open_node`; `Main` outside `Ui::layer` scopes.
    #[inline]
    pub(crate) fn cur_plane(&self) -> Plane {
        self.plane_stack.last().copied().unwrap_or(Plane::MAIN)
    }

    /// Recorded nodes across every plane.
    pub(crate) fn total_nodes(&self) -> usize {
        self.trees.iter().map(|tree| tree.records.len()).sum()
    }

    /// Top-level roots across every plane.
    pub(crate) fn total_roots(&self) -> usize {
        self.trees.iter().map(|tree| tree.roots.len()).sum()
    }

    /// Interns a grid's track definition into the current plane's tree, so `open_node`'s handle check holds by construction.
    #[inline]
    pub(crate) fn push_grid_def(&mut self, rows: &[Track], cols: &[Track]) -> GridDefId {
        let plane = self.cur_plane();
        self.trees[plane].push_grid_def(rows, cols)
    }

    /// Interns a bar overlay's definition into the current plane's tree, with its viewport resolved to the node recorded this pass.
    ///
    /// Probes the id map being filled, so the viewport must already be open.
    ///
    /// # Panics
    ///
    /// Panics if the viewport was not recorded earlier this frame.
    #[inline]
    pub(crate) fn push_scrollbars_def(&mut self, def: ScrollbarsDef) -> ScrollbarsDefId {
        let plane = self.cur_plane();
        let Some(viewport) = self.ids.endpoint(def.content) else {
            panic!(
                "scrollbar overlay names viewport {:?}, which was not recorded earlier this frame",
                def.content,
            );
        };
        // The driver reads the viewport's extent out of its own plane's
        // table, so a node from another plane would index the wrong one.
        debug_assert_eq!(
            viewport.plane, plane,
            "a scrollbar overlay and its viewport must record on one plane",
        );
        let content = viewport.node;
        self.trees[plane].push_scrollbars_def(ResolvedScrollbarsDef { def, content })
    }

    /// Resolves `ident` against the open parent into the id this frame records under.
    ///
    /// Parent mixing and disambiguation must happen together: without the first the id moves with record order, without the second it collides. Parent-scoped idents pass their inputs to the tracker so an unchanged frame never hashes the raw id.
    #[inline]
    pub(crate) fn widget_id(&mut self, ident: Ident) -> ResolvedId {
        let parent = self.cur_parent_id();
        match ident {
            Ident::Auto(_) | Ident::Hash(_) => self.ids.resolve_scoped(ident, parent),
            _ => self.ids.resolve(ident.raw_id(parent), ident.kind()),
        }
    }

    pub(crate) fn pre_record(&mut self) {
        self.record_store.clear();
        self.plane_stack.clear();
        self.ids.pre_record();
        self.collisions.clear();
        for tree in &mut self.trees {
            tree.pre_record();
        }
        for scratch in &mut self.scratch {
            scratch.clear();
        }
    }

    /// Finalizes every tree.
    pub(crate) fn post_record(&mut self) {
        tracy::zone!();
        let active = self.cur_plane();
        debug_assert_eq!(
            active,
            Plane::MAIN,
            "post_record called with active plane {active:?} — Ui::layer body forgot to return",
        );
        for (tree, scratch) in self.trees.iter_mut().zip(&self.scratch) {
            debug_assert!(
                scratch.open_frames.is_empty(),
                "post_record: a plane has {} node(s) still open — a widget builder forgot close_node",
                scratch.open_frames.len(),
            );
            tree.post_record();
        }
    }

    /// Minimum `next_wake` across every plane's paint anims, or `None` when nothing wants a wake.
    pub(crate) fn min_paint_animation_wake(&self, now: Duration) -> Option<Duration> {
        self.trees
            .iter()
            .flat_map(|tree| &tree.paint_animations.entries)
            .filter_map(|entry| entry.animation.next_wake(now))
            .min()
    }

    /// Opens a node whose id was already resolved by [`crate::widget::Widget::resolve`], and records its endpoint (also emitting any pending explicit collision pair).
    #[inline]
    pub(crate) fn open_node(
        &mut self,
        resolved: ResolvedId,
        node: &Node,
        node_chrome: NodeChrome<'_>,
    ) {
        let plane = self.cur_plane();
        let store = &mut self.record_store;
        let NodeChrome { chrome, ring } = node_chrome;
        // A ring with no chrome rides an empty one.
        let input = (chrome.is_some() || !ring.is_noop()).then(|| ChromeInput {
            chrome: chrome.unwrap_or(&Chrome::NONE),
            ring,
            store,
        });
        let tree = &mut self.trees[plane];
        let scratch = &mut self.scratch[plane];
        let node_id = tree.open_node(scratch, node.cols(resolved.id()), input);
        let endpoint = Endpoint {
            plane,
            node: node_id,
        };
        if let Some(collision) = self.ids.record_endpoint(resolved, endpoint) {
            self.report_explicit_collision(collision);
        }
    }

    /// Outlined from [`Self::open_node`]: the `tracing::error!` expansion would enlarge its stack frame.
    #[cold]
    #[inline(never)]
    fn report_explicit_collision(&mut self, collision: CollisionRecord) {
        let CollisionRecord { first, second } = collision;
        tracing::error!(
            first_plane = ?first.plane,
            first_node = ?first.node,
            second_plane = ?second.plane,
            second_node = ?second.node,
            "explicit WidgetId collision — disambiguated; per-widget state will not survive between the colliding call sites",
        );
        self.collisions.push(collision);
    }

    #[inline]
    pub(crate) fn close_node(&mut self) {
        let plane = self.cur_plane();
        let tree = &mut self.trees[plane];
        let scratch = &mut self.scratch[plane];
        tree.close_node(scratch);
    }

    /// Gate for the `add_*` entry points: a shape needs an open node.
    fn assert_node_open(&self, plane: Plane, what: &str) {
        debug_assert!(
            !self.scratch[plane].open_frames.is_empty(),
            "{what} called with no open node",
        );
    }

    /// Whether a record pass is in flight, i.e. some plane has a node open.
    ///
    /// Not `cur_plane()`: [`Ui::layer`](crate::Ui::layer) pushes a layer without opening a node in it, which would misread as not recording.
    pub(crate) fn is_recording(&self) -> bool {
        self.scratch.iter().any(|s| !s.open_frames.is_empty())
    }

    /// Lowers a user-facing [`Shape`](crate::widget::Shape) and appends it to the active tree's shape buffer.
    pub(crate) fn add_shape<S: Lower>(&mut self, shape: S) {
        self.push_shape("add_shape", |tree, store, _| {
            tree.shapes.add(shape, store).is_some()
        });
    }

    /// Appends a `GpuView` shape to the active node. Only the redraw `epoch` rides the shape; it skips lowering and never noop-collapses.
    pub(crate) fn add_gpu_view(&mut self, epoch: u64) {
        self.push_shape("add_gpu_view", |tree, _, _| {
            tree.shapes.add_gpu_view(epoch);
            true
        });
    }

    /// [`Self::add_shape`] plus a `PaintAnimation` registered against the new shape. No entry is pushed if the shape noop-collapsed or the node is effectively invisible.
    pub(crate) fn add_shape_animated<S: Lower>(&mut self, shape: S, animation: PaintAnimation) {
        self.push_shape("add_shape_animated", |tree, store, frame| {
            let Some(shape_idx) = tree.shapes.add(shape, store) else {
                return false;
            };
            tree.shapes.fold_paint_animation(shape_idx, &animation);
            // The paint row is charged either way.
            if frame.effectively_visible {
                tree.paint_animations.push_entry(PaintAnimationEntry {
                    animation,
                    shape_idx,
                    row: frame.paint_rows,
                    node: frame.node,
                });
            }
            true
        });
    }

    /// Shared body of the `add_*` entry points: gates on an open node, runs `push`, and charges one paint row if `push` returns `true`.
    ///
    /// `push` sees the frame before the bump, so an animation row is stamped with the row its shape takes.
    #[inline]
    fn push_shape(
        &mut self,
        what: &str,
        push: impl FnOnce(&mut Tree, &mut RecordStore, &OpenFrame) -> bool,
    ) {
        let plane = self.cur_plane();
        self.assert_node_open(plane, what);
        let tree = &mut self.trees[plane];
        let frames = &mut self.scratch[plane].open_frames;
        let frame = frames
            .last_mut()
            .expect("`assert_node_open` above found an open frame");
        if push(tree, &mut self.record_store, frame) {
            frame.paint_rows += 1;
        }
    }

    /// Open a side-layer scope for an overlay asking for `layer`. It records in the higher of
    /// `layer` and the current plane's layer, one plane above the current one when they match, so
    /// it paints above whatever opened it.
    pub(crate) fn push_layer(&mut self, layer: Layer, placement: Placement) {
        let active = self.cur_plane();
        let plane = Self::plane_above(active, layer);
        self.trees.ensure(plane);
        self.scratch.ensure(plane);
        let owner_disabled = self.scratch[active].ancestor_disabled();
        let scratch = &mut self.scratch[plane];
        debug_assert!(
            scratch.open_frames.is_empty(),
            "Ui::layer({layer:?}) chose {plane:?}, which has a node open",
        );
        scratch.pending_placement = Some(placement);
        scratch.pending_layer = layer;
        scratch.owner_disabled = owner_disabled;
        self.plane_stack.push(plane);
    }

    /// The plane an overlay asking for `layer` opened from `active` records into.
    fn plane_above(active: Plane, layer: Layer) -> Plane {
        if layer > active.layer {
            return Plane::base(layer);
        }
        let depth = active.depth.checked_add(1).expect(
            "Ui::layer nested past 255 overlays in one layer — an overlay opening itself in its own body?",
        );
        Plane {
            layer: active.layer,
            depth,
        }
    }

    pub(crate) fn pop_layer(&mut self) {
        let plane = self
            .plane_stack
            .pop()
            .expect("pop_layer without matching push_layer");
        let scratch = &mut self.scratch[plane];
        debug_assert!(
            scratch.open_frames.is_empty(),
            "Ui::layer body left {} node(s) open in {:?}",
            scratch.open_frames.len(),
            plane,
        );
        scratch.pending_placement = None;
        scratch.pending_layer = Layer::Main;
        scratch.owner_disabled = false;
    }

    #[inline]
    fn cur_tree(&self) -> &Tree {
        &self.trees[self.cur_plane()]
    }

    #[inline]
    fn cur_scratch(&self) -> &RecordingScratch {
        &self.scratch[self.cur_plane()]
    }

    /// Whether an ancestor of the node being recorded is disabled; the cascade reports it a frame late.
    #[inline]
    pub(crate) fn ancestor_disabled(&self) -> bool {
        self.cur_scratch().ancestor_disabled()
    }

    /// `WidgetId` of the innermost open node in the active plane, or `None` before any.
    #[inline]
    pub(crate) fn cur_parent_id(&self) -> Option<WidgetId> {
        let tree = self.cur_tree();
        self.cur_scratch()
            .open_frames
            .last()
            .map(|f| tree.records.widget_id()[f.node.idx()])
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::scene::forest::Forest;
    use crate::scene::plane::Plane;
    use crate::scene::tree::node_id::NodeId;
    use crate::scene::tree::recording_scratch::RecordingScratch;

    impl Forest {
        /// Record's ancestor stacks and plane stack; the trees, payloads, ids and collisions are what record hands the later phases.
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                trees: _,
                record_store: _,
                scratch,
                ids: _,
                collisions: _,
                plane_stack,
            } = self;
            let open_frames: usize = scratch
                .iter()
                .map(
                    |RecordingScratch {
                         open_frames,
                         pending_placement: _,
                         pending_layer: _,
                         owner_disabled: _,
                     }| scratch_report::vec_bytes(open_frames),
                )
                .sum();
            open_frames + scratch_report::vec_bytes(plane_stack)
        }

        /// The node carrying `id` on `plane`; linear scan.
        pub(crate) fn node_for_widget_id(&self, plane: impl Into<Plane>, id: WidgetId) -> NodeId {
            let idx = self.trees[plane.into()]
                .records
                .widget_id()
                .iter()
                .position(|widget_id| *widget_id == id)
                .unwrap_or_else(|| panic!("no node found for widget_id {id:?}"));
            NodeId(idx as u32)
        }
    }
}
