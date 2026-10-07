//! The recorded half of the scene: one arena per layer, plus the per-frame
//! identity tracker and layer stack that recording needs.

use crate::common::tracy;
use crate::layout::drivers::scrollbars::scrollbars_def::{ResolvedScrollbarsDef, ScrollbarsDef};
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::layout_mode::{GridDefId, ScrollbarsDefId};
use crate::primitives::layout::placement::Placement;
use crate::primitives::layout::track::Track;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::endpoint::Endpoint;
use crate::scene::layer::Layer;
use crate::scene::node::Node;
use crate::scene::node::ident::Ident;
use crate::scene::per_layer::PerLayer;
use crate::scene::record_store::RecordStore;
use crate::scene::seen_ids::{CollisionRecord, ResolvedId, SeenIds};
use crate::scene::tree::ChromeInput;
use crate::scene::tree::Tree;
use crate::scene::tree::paint_anims::PaintAnimEntry;
use crate::scene::tree::paint_anims::paint_animation::PaintAnimation;
use crate::scene::tree::recording_scratch::OpenFrame;
use crate::scene::tree::recording_scratch::RecordingScratch;
use crate::shape::Lower;
use std::time::Duration;

/// One arena per [`Layer`]; recording dispatches to `trees[current_layer.idx()]`.
#[derive(Debug, Default)]
pub(crate) struct Forest {
    pub(crate) trees: PerLayer<Tree>,
    /// Variable-sized payloads referenced by shape records; cleared on a record pass, retained across `PaintOnly` frames.
    pub(crate) record_store: RecordStore,
    /// Per-layer recording-only state, kept off `Tree` so downstream passes can't reach it. Cleared by `pre_record`.
    scratch: PerLayer<RecordingScratch>,
    /// Per-frame `WidgetId` tracker (collision detection, auto-id disambiguation). Lives here so every path to `open_node` gets the check.
    pub(crate) ids: SeenIds,
    /// Explicit-id collisions this frame, read by `encoder::emit_collision_overlays`.
    ///
    /// Recorded in every profile for tests, but only painted in a development build.
    pub(crate) collisions: Vec<CollisionRecord>,
    /// Active side-layer scopes; empty for the `Main` baseline.
    ///
    /// A nested layer must rank strictly above its parent (`push_layer` asserts): paint and hit order is `Layer::PAINT_ORDER` with no per-node z. That also keeps each layer on the stack at most once, so `pending_placement` is single-occupancy.
    layer_stack: Vec<Layer>,
}

impl Forest {
    /// Active layer for the next `open_node`; `Main` outside `Ui::layer` scopes.
    #[inline]
    pub(crate) fn current_layer(&self) -> Layer {
        self.layer_stack.last().copied().unwrap_or(Layer::Main)
    }

    /// Recorded nodes across every layer.
    pub(crate) fn total_nodes(&self) -> usize {
        self.trees.iter().map(|tree| tree.records.len()).sum()
    }

    /// Top-level roots across every layer.
    pub(crate) fn total_roots(&self) -> usize {
        self.trees.iter().map(|tree| tree.roots.len()).sum()
    }

    /// Interns a grid's track definition into the current layer's tree, so `open_node`'s handle check holds by construction.
    #[inline]
    pub(crate) fn push_grid_def(&mut self, rows: &[Track], cols: &[Track]) -> GridDefId {
        let layer = self.current_layer();
        self.trees[layer].push_grid_def(rows, cols)
    }

    /// Interns a bar overlay's definition into the current layer's tree, with its viewport resolved to the node recorded this pass.
    ///
    /// Probes the id map being filled, so the viewport must already be open.
    ///
    /// # Panics
    ///
    /// Panics if the viewport was not recorded earlier this frame.
    #[inline]
    pub(crate) fn push_scrollbars_def(&mut self, def: ScrollbarsDef) -> ScrollbarsDefId {
        let layer = self.current_layer();
        let Some(viewport) = self.ids.endpoint(def.content) else {
            panic!(
                "scrollbar overlay names viewport {:?}, which was not recorded earlier this frame",
                def.content,
            );
        };
        // The driver reads the viewport's extent out of its own layer's
        // table, so a node from another layer would index the wrong one.
        debug_assert_eq!(
            viewport.layer, layer,
            "a scrollbar overlay and its viewport must record on one layer",
        );
        let content = viewport.node;
        self.trees[layer].push_scrollbars_def(ResolvedScrollbarsDef { def, content })
    }

    /// Resolves `ident` against the open parent into the id this frame records under.
    ///
    /// Parent mixing and disambiguation must happen together: without the first the id moves with record order, without the second it collides. Parent-scoped idents pass their inputs to the tracker so an unchanged frame never hashes the raw id.
    #[inline]
    pub(crate) fn widget_id(&mut self, ident: Ident) -> ResolvedId {
        let parent = self.current_parent_id();
        match ident {
            Ident::Auto(_) | Ident::Hash(_) => self.ids.resolve_scoped(ident, parent),
            _ => self.ids.resolve(ident.raw_id(parent), ident.is_explicit()),
        }
    }

    pub(crate) fn pre_record(&mut self) {
        self.record_store.clear();
        self.layer_stack.clear();
        self.ids.pre_record();
        self.collisions.clear();
        for t in self.trees.iter_mut() {
            t.pre_record();
        }
        for s in self.scratch.iter_mut() {
            s.clear();
        }
    }

    /// Finalizes every tree.
    pub(crate) fn post_record(&mut self) {
        tracy::zone!();
        let active = self.current_layer();
        debug_assert_eq!(
            active,
            Layer::Main,
            "post_record called with active layer {active:?} — Ui::layer body forgot to return",
        );
        for layer in Layer::PAINT_ORDER {
            let scratch = &self.scratch[layer];
            debug_assert!(
                scratch.open_frames.is_empty(),
                "post_record: layer {layer:?} has {} node(s) still open — a widget builder forgot close_node",
                scratch.open_frames.len(),
            );
            self.trees[layer].post_record();
        }
    }

    /// Minimum `next_wake` across every layer's paint anims, or `None` when nothing wants a wake.
    pub(crate) fn min_paint_anim_wake(&self, now: Duration) -> Option<Duration> {
        self.trees
            .iter()
            .flat_map(|tree| &tree.paint_anims.entries)
            .filter_map(|entry| entry.anim.next_wake(now))
            .min()
    }

    /// Opens a node whose id was already resolved by [`crate::widget::Widget::resolve`], and records its endpoint (also emitting any pending explicit collision pair).
    ///
    /// `chrome` is borrowed so a chromed widget doesn't re-copy its `Background` down the record chain.
    #[inline]
    pub(crate) fn open_node(
        &mut self,
        resolved: ResolvedId,
        node: &Node,
        chrome: Option<&Background>,
        ring: Stroke,
    ) {
        let layer = self.current_layer();
        let store = &mut self.record_store;
        // A ring with no background rides an empty one.
        let chrome = (chrome.is_some() || !ring.is_noop()).then(|| ChromeInput {
            bg: chrome.unwrap_or(&Background::NONE),
            ring,
            store,
        });
        let tree = &mut self.trees[layer];
        let scratch = &mut self.scratch[layer];
        let node_id = tree.open_node(scratch, resolved.id(), node, chrome);
        let endpoint = Endpoint {
            layer,
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
            first_layer = ?first.layer,
            first_node = ?first.node,
            second_layer = ?second.layer,
            second_node = ?second.node,
            "explicit WidgetId collision — disambiguated; per-widget state will not survive between the colliding call sites",
        );
        self.collisions.push(collision);
    }

    #[inline]
    pub(crate) fn close_node(&mut self) {
        let layer = self.current_layer();
        let tree = &mut self.trees[layer];
        let scratch = &mut self.scratch[layer];
        tree.close_node(scratch);
    }

    /// Gate for the `add_*` entry points: a shape needs an open node.
    fn assert_node_open(&self, layer: Layer, what: &str) {
        debug_assert!(
            !self.scratch[layer].open_frames.is_empty(),
            "{what} called with no open node",
        );
    }

    /// Whether a record pass is in flight, i.e. some layer has a node open.
    ///
    /// Not `current_layer()`: [`Ui::layer`](crate::Ui::layer) pushes a layer without opening a node in it, which would misread as not recording.
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
    pub(crate) fn add_shape_animated<S: Lower>(&mut self, shape: S, anim: PaintAnimation) {
        self.push_shape("add_shape_animated", |tree, store, frame| {
            let Some(shape_idx) = tree.shapes.add(shape, store) else {
                return false;
            };
            tree.shapes.fold_paint_anim(shape_idx, &anim);
            // The paint row is charged either way.
            if frame.effectively_visible {
                tree.paint_anims.push_entry(PaintAnimEntry {
                    anim,
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
        let layer = self.current_layer();
        self.assert_node_open(layer, what);
        let tree = &mut self.trees[layer];
        let frames = &mut self.scratch[layer].open_frames;
        let frame = frames
            .last_mut()
            .expect("`assert_node_open` above found an open frame");
        if push(tree, &mut self.record_store, frame) {
            frame.paint_rows += 1;
        }
    }

    pub(crate) fn push_layer(&mut self, layer: Layer, placement: Placement) {
        let active = self.current_layer();
        // A nested layer must paint above the scope it's raised from (e.g. Tooltip over Popup), and equal would clobber `pending_placement`.
        //
        // Release assert: a violation raises no error downstream, the frame is just subtly wrong. Cold path, once per side scope.
        assert!(
            layer > active,
            "Ui::layer({layer:?}) must rank above the current scope ({active:?}) \
             in Layer::PAINT_ORDER — a nested layer painting under its parent is a bug",
        );
        let owner_disabled = self.scratch[active].ancestor_disabled();
        let scratch = &mut self.scratch[layer];
        debug_assert!(
            scratch.open_frames.is_empty(),
            "Ui::layer({layer:?}) called while a node is still open in that layer",
        );
        scratch.pending_placement = Some(placement);
        scratch.owner_disabled = owner_disabled;
        self.layer_stack.push(layer);
    }

    pub(crate) fn pop_layer(&mut self) {
        let layer = self
            .layer_stack
            .pop()
            .expect("pop_layer without matching push_layer");
        let scratch = &mut self.scratch[layer];
        debug_assert!(
            scratch.open_frames.is_empty(),
            "Ui::layer body left {} node(s) open in layer {:?}",
            scratch.open_frames.len(),
            layer,
        );
        scratch.pending_placement = None;
        scratch.owner_disabled = false;
    }

    #[inline]
    fn current_tree(&self) -> &Tree {
        &self.trees[self.current_layer()]
    }

    #[inline]
    fn current_scratch(&self) -> &RecordingScratch {
        &self.scratch[self.current_layer()]
    }

    /// Whether an ancestor of the node being recorded is disabled; the cascade reports it a frame late.
    #[inline]
    pub(crate) fn ancestor_disabled(&self) -> bool {
        self.current_scratch().ancestor_disabled()
    }

    /// `WidgetId` of the innermost open node in the active layer, or `None` before any.
    #[inline]
    pub(crate) fn current_parent_id(&self) -> Option<WidgetId> {
        let tree = self.current_tree();
        self.current_scratch()
            .open_frames
            .last()
            .map(|f| tree.records.widget_id()[f.node.idx()])
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::scene::forest::Forest;
    use crate::scene::layer::Layer;
    use crate::scene::tree::node_id::NodeId;

    impl Forest {
        /// The node carrying `id` on `layer`; linear scan.
        pub(crate) fn node_for_widget_id(&self, layer: Layer, id: WidgetId) -> NodeId {
            let idx = self.trees[layer]
                .records
                .widget_id()
                .iter()
                .position(|widget_id| *widget_id == id)
                .unwrap_or_else(|| panic!("no node found for widget_id {id:?}"));
            NodeId(idx as u32)
        }
    }
}
