//! Recorded and derived scene state. [`Forest`] owns one arena per
//! layer; cascade and damage turn that recording into the immutable
//! per-frame data consumed by input and rendering. [`record_store`]
//! retains the variable-sized payloads referenced by recorded shapes.

use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::{Layer, PerLayer};
use crate::scene::node::Node;
use crate::scene::record_store::RecordStore;
use crate::scene::seen_ids::{CollisionRecord, Endpoint, EndpointOutcome, SeenIds};
use crate::scene::shapes::lower::ChromeInput;
use crate::scene::tree::Tree;
use crate::scene::tree::paint_anims::{PaintAnim, PaintAnimEntry};
use crate::scene::tree::recording::{Placement, RecordingScratch};
use crate::shape::Shape;
use std::time::Duration;

pub(crate) mod cascade;
pub(crate) mod damage;
pub(crate) mod layer;
pub(crate) mod node;
pub(crate) mod record_store;
pub(crate) mod seen_ids;
pub(crate) mod shapes;
pub(crate) mod tree;
pub(crate) mod visibility;

/// One arena per [`Layer`]. Recording dispatches `open_node`,
/// `add_shape`, `close_node` to `trees[current_layer.idx()]`.
/// Pipeline passes iterate trees via [`Forest::iter_paint_order`];
/// known-layer access indexes `trees[layer]` directly.
#[derive(Debug, Default)]
pub(crate) struct Forest {
    pub(crate) trees: PerLayer<Tree>,
    /// Variable-sized payloads referenced by shape records in `trees`.
    /// Cleared with the trees on a record pass and retained with them across
    /// `PaintOnly` frames.
    pub(crate) record_store: RecordStore,
    /// Per-layer recording-only state (ancestor stack + pending
    /// anchor). Lives off `Tree` so downstream passes holding `&Tree`
    /// can't reach transient state; cleared by `pre_record`, drained
    /// at each top-level `close_node`. Disjoint from `trees` so
    /// `open_node` can borrow both via field access.
    pub(crate) scratch: PerLayer<RecordingScratch>,
    /// Per-frame `WidgetId` tracker. Mutated by `open_node` (collision
    /// detection + auto-id disambiguation), reset by `pre_record`, and
    /// rolled over by `Ui::finalize_frame` (which fans `ids.removed`
    /// out to per-widget caches). Lives on `Forest` so any path that
    /// reaches `open_node` — including direct callers that bypass
    /// `Widget::record` — gets the same collision check.
    pub(crate) ids: SeenIds,
    /// Explicit-id collisions recorded this frame — each carries the
    /// first-occurrence and disambiguated nodes (with their layers).
    /// Read by `encoder::emit_collision_overlays` after the regular
    /// paint walk; cleared by the next `pre_record`. Public-in-crate
    /// so tests can introspect.
    pub(crate) collisions: Vec<CollisionRecord>,
    /// Stack of active side-layer scopes; empty for the `Main` baseline.
    /// `push_layer` pushes, `pop_layer` pops and restores the parent
    /// scope. A nested layer must rank strictly higher than the scope it
    /// opens from (`push_layer` asserts `layer > current`) — the
    /// cross-layer paint/hit order is `Layer::PAINT_ORDER` with no
    /// per-node z, so a lower nest would paint under its parent. Real
    /// case: a tooltip rising from a popup or modal body. Strictly
    /// increasing ⇒ each layer appears at most once, keeping the
    /// per-`Tree` `pending_placement` slot single-occupancy. Retained across
    /// frames (cleared with capacity kept in `pre_record`) so
    /// steady-state recording is alloc-free.
    layer_stack: Vec<Layer>,
}

impl Forest {
    /// Active layer for the next `open_node`. `Main` between/outside
    /// `Ui::layer` scopes; switched by `push_layer` / `pop_layer`.
    #[inline]
    pub(crate) fn current_layer(&self) -> Layer {
        self.layer_stack.last().copied().unwrap_or(Layer::Main)
    }

    pub(crate) fn pre_record(&mut self) {
        self.record_store.clear();
        self.layer_stack.clear();
        self.ids.pre_record();
        self.collisions.clear();
        for t in &mut self.trees {
            t.pre_record();
        }
        for s in &mut self.scratch {
            s.clear();
        }
    }

    /// Finalize every tree. Pure structural pass — the surface needed
    /// to evaluate each root's placement is passed to `LayoutEngine::run`.
    /// The paint-anim wake fold is centralised in
    /// [`Self::min_paint_anim_wake`] and run at the tail of
    /// `Ui::frame` for both record + paint-only paths.
    #[profiling::function]
    pub(crate) fn post_record(&mut self) {
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

    /// Minimum `next_wake` across every layer's paint anims, or `None`
    /// when nothing wants a wake. Called from `Ui::frame` after both
    /// record and paint-only paths so the next anim boundary is queued
    /// regardless of which path ran.
    pub(crate) fn min_paint_anim_wake(&self, now: Duration) -> Option<Duration> {
        (&self.trees)
            .into_iter()
            .flat_map(|tree| &tree.paint_anims.entries)
            .map(|entry| entry.anim.next_wake(now))
            .min()
    }

    /// Open a node whose id has already been resolved + disambiguated
    /// upstream by [`crate::Ui::widget`] (which calls
    /// `SeenIds::resolve` eagerly so the returned id matches what the
    /// tree, cascade, and `response_for` see). This function takes
    /// the id verbatim, opens the node in the active tree, and records
    /// the endpoint the tree assigned via `SeenIds::record_endpoint`
    /// (also emitting any pending explicit collision pair).
    ///
    /// `chrome` is `Some(Background { .. })` for nodes with a background
    /// paint and `None` otherwise. The `Background` is borrowed (not
    /// owned) so its 168 B don't get copied through the
    /// `Widget::record → here → Tree::open_node →
    /// shapes::lower::background` chain on every chromed widget.
    #[inline]
    pub(crate) fn open_node(
        &mut self,
        widget_id: WidgetId,
        node: Node,
        chrome: Option<&Background>,
    ) {
        let layer = self.current_layer();
        let chrome = chrome.map(|bg| ChromeInput {
            bg,
            store: &self.record_store,
        });
        // Disjoint borrow: record storage, `trees`, and `scratch` are separate
        // fields, so all three can be borrowed for the same call.
        let tree = &mut self.trees[layer];
        let scratch = &mut self.scratch[layer];
        let node_id = tree.open_node(scratch, widget_id, node, chrome);
        let endpoint = Endpoint {
            layer,
            node: node_id,
        };
        if let EndpointOutcome::ExplicitCollision { first, second } =
            self.ids.record_endpoint(widget_id, endpoint)
        {
            self.report_explicit_collision(first, second);
        }
    }

    /// Outlined from [`Self::open_node`]: the `tracing::error!` expansion
    /// reserves stack slots in whatever function it inlines into, taxing
    /// every open with a bigger frame for a path that fires only on a
    /// caller bug.
    #[cold]
    #[inline(never)]
    fn report_explicit_collision(&mut self, first: Endpoint, second: Endpoint) {
        tracing::error!(
            first_layer = ?first.layer,
            first_node = ?first.node,
            second_layer = ?second.layer,
            second_node = ?second.node,
            "explicit WidgetId collision — disambiguated; per-widget state will not survive between the colliding call sites",
        );
        self.collisions.push(CollisionRecord { first, second });
    }

    pub(crate) fn close_node(&mut self) {
        let layer = self.current_layer();
        let tree = &mut self.trees[layer];
        let scratch = &mut self.scratch[layer];
        tree.close_node(scratch);
    }

    /// Shared gate for the `add_*` recording entry points: a shape can
    /// only attach to a currently-open node, so widgets can't leak
    /// shapes outside an `open_node` / `close_node` scope.
    fn assert_node_open(&self, layer: Layer, what: &str) {
        debug_assert!(
            !self.scratch[layer].open_frames.is_empty(),
            "{what} called with no open node",
        );
    }

    /// Lower a user-facing [`Shape`] (curve flattening, span
    /// stamping, hashing) and append it to the active tree's shape
    /// buffer. Asserts a node is currently open so widgets can't leak
    /// shapes outside an `open_node` / `close_node` scope.
    pub(crate) fn add_shape(&mut self, shape: Shape<'_>) {
        let layer = self.current_layer();
        self.assert_node_open(layer, "add_shape");
        // Static shapes must not pay a sentinel push into the sparse registry.
        if self.trees[layer]
            .shapes
            .add(shape, &self.record_store)
            .is_some()
        {
            self.scratch[layer]
                .open_frames
                .last_mut()
                .unwrap()
                .paint_rows += 1;
        }
    }

    /// Append a `GpuView` shape (a [`ShapeRecord::GpuView`]) to the active
    /// node. Only the redraw `epoch` rides the shape — the view's `id` + app
    /// `paint` live in `Ui::gpu_views` keyed by the owner's `WidgetId`; this is
    /// assembled by `Ui::gpu_view`, not lowered from a user-facing [`Shape`],
    /// so it skips the lowering path.
    pub(crate) fn add_gpu_view(&mut self, epoch: u64) {
        let layer = self.current_layer();
        self.assert_node_open(layer, "add_gpu_view");
        self.trees[layer].shapes.add_gpu_view(epoch);
        self.scratch[layer]
            .open_frames
            .last_mut()
            .unwrap()
            .paint_rows += 1;
    }

    /// Same as `add_shape`, but registers a `PaintAnim` against the
    /// freshly-pushed shape so the encoder applies the sampled
    /// `PaintMod` at paint time and `post_record` folds the anim's
    /// `next_wake` into the host's repaint queue. Drops silently
    /// (no entry pushed) if the shape itself was noop-collapsed.
    /// Effectively invisible shapes stay authored but omit their
    /// animation row until a visible record pass resumes them.
    pub(crate) fn add_shape_animated(&mut self, shape: Shape<'_>, anim: PaintAnim) {
        let layer = self.current_layer();
        self.assert_node_open(layer, "add_shape_animated");
        // Disjoint borrow: `trees` and `scratch` are separate fields.
        let tree = &mut self.trees[layer];
        let frame = self.scratch[layer].open_frames.last_mut().unwrap();
        let Some(shape_idx) = tree.shapes.add(shape, &self.record_store) else {
            return;
        };
        let row = frame.paint_rows;
        frame.paint_rows += 1;
        if !frame.effectively_visible {
            return;
        }
        tree.paint_anims.push_entry(
            shape_idx,
            PaintAnimEntry {
                anim,
                row,
                node_idx: frame.node.0,
            },
        );
    }

    pub(crate) fn push_layer(&mut self, layer: Layer, placement: Placement) {
        let active = self.current_layer();
        // A nested side layer must paint *above* the scope it's raised
        // from. The cross-layer scheme has no per-node z-index — paint
        // and hit order are entirely `Layer::PAINT_ORDER` — so `layer`
        // must rank strictly higher than the active scope. This admits
        // the real cases (a tooltip rising from a popup or modal body:
        // Tooltip > Popup, Tooltip > Modal) and rejects a lower-or-equal
        // nest, which would record fine but then render *underneath* its
        // parent (occluded, un-hittable). Equal is rejected too: it would
        // also clobber the single per-layer `pending_placement` slot.
        // Strictly increasing ⇒ each layer appears at most once on the
        // stack, so that slot stays single-occupancy without a guard.
        debug_assert!(
            layer > active,
            "Ui::layer({layer:?}) must rank above the current scope ({active:?}) \
             in Layer::PAINT_ORDER — a nested layer painting under its parent is a bug",
        );
        let scratch = &mut self.scratch[layer];
        debug_assert!(
            scratch.open_frames.is_empty(),
            "Ui::layer({layer:?}) called while a node is still open in that layer",
        );
        scratch.pending_placement = Some(placement);
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
    }

    /// Borrow the tree for the [`Self::current_layer`] — the one
    /// `open_node` / `add_shape` dispatch to. Convenience over
    /// `tree(current_layer())` for the very common case.
    #[inline]
    pub(crate) fn current_tree(&self) -> &Tree {
        &self.trees[self.current_layer()]
    }

    /// Recording-only scratch for the active layer. Read by
    /// [`Self::current_parent_id`] and the disabled cascade at record
    /// time.
    #[inline]
    pub(crate) fn current_scratch(&self) -> &RecordingScratch {
        &self.scratch[self.current_layer()]
    }

    /// `WidgetId` of the innermost open node in the active layer — the
    /// parent context auto/salted ids resolve against (`Ui::widget`)
    /// — or `None` at the top of a layer with no node open yet.
    #[inline]
    pub(crate) fn current_parent_id(&self) -> Option<WidgetId> {
        let tree = self.current_tree();
        self.current_scratch()
            .open_frames
            .last()
            .map(|f| tree.records.widget_id()[f.node.idx()])
    }
}
