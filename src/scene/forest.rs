//! The recorded half of the scene: one arena per layer, plus the
//! per-frame identity tracker and layer stack that recording needs.
//! `cascade` and `damage` turn what lands here into the immutable
//! per-frame data input and rendering read.

use crate::common::tracy;
use crate::layout::scrollbars::ScrollbarsDef;
use crate::layout::types::layout_mode::{GridDefId, ScrollbarsDefId};
use crate::layout::types::placement::Placement;
use crate::layout::types::track::Track;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::{Layer, PerLayer};
use crate::scene::node::Node;
use crate::scene::node::salt::Salt;
use crate::scene::record_store::RecordStore;
use crate::scene::seen_ids::{CollisionRecord, Endpoint, SeenIds};
use crate::scene::tree::ChromeInput;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_anims::{PaintAnim, PaintAnimEntry};
use crate::scene::tree::recording_scratch::RecordingScratch;
use crate::shape::Lower;
use std::time::Duration;

/// One arena per [`Layer`]. Recording dispatches `open_node`,
/// `add_shape`, `close_node` to `trees[current_layer.idx()]`.
/// Pipeline passes iterate trees via [`PerLayer::iter_paint_order`];
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
    /// rolled over by `FrameCycle::finalize_frame` (which fans `ids.removed`
    /// out to per-widget caches). Lives on `Forest` so any path that
    /// reaches `open_node` — including direct callers that bypass
    /// `Widget::record` — gets the same collision check.
    pub(crate) ids: SeenIds,
    /// Explicit-id collisions recorded this frame — each carries the
    /// first-occurrence and disambiguated nodes (with their layers).
    /// Read by `encoder::emit_collision_overlays` after the regular
    /// paint walk; cleared by the next `pre_record`. Public-in-crate
    /// so tests can introspect.
    ///
    /// Recorded in every profile so the `internals` harness can assert
    /// on it, but only *painted* in a development build — see
    /// `encoder::emit_collision_overlays`.
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

    /// Recorded nodes across every layer. The cascade sizes its flat
    /// per-node tables against this and the measure cache checks its
    /// snapshot against it, so it is one method rather than the same
    /// fold spelled three ways.
    pub(crate) fn total_nodes(&self) -> usize {
        self.trees.iter().map(|tree| tree.records.len()).sum()
    }

    /// Top-level roots across every layer — the companion count to
    /// [`Self::total_nodes`], read by the measure cache's snapshot key.
    pub(crate) fn total_roots(&self) -> usize {
        self.trees.iter().map(|tree| tree.roots.len()).sum()
    }

    /// Intern a grid's track definition into the current layer's tree.
    /// The returned handle is what a `Node::grid` packs, and `open_node`
    /// debug-asserts that a grid node's handle resolves — pushing
    /// through the active layer here is what makes that hold by
    /// construction rather than by every caller remembering.
    #[inline]
    pub(crate) fn push_grid_def(
        &mut self,
        rows: &[Track],
        cols: &[Track],
        row_gap: f32,
        col_gap: f32,
    ) -> GridDefId {
        let layer = self.current_layer();
        self.trees[layer].push_grid_def(rows, cols, row_gap, col_gap)
    }

    /// Intern a bar overlay's definition into the current layer's tree.
    /// Companion to [`Self::push_grid_def`]; same layer contract.
    #[inline]
    pub(crate) fn push_scrollbars_def(&mut self, def: ScrollbarsDef) -> ScrollbarsDefId {
        let layer = self.current_layer();
        self.trees[layer].push_scrollbars_def(def)
    }

    /// Resolve `salt` against the currently-open parent into the id this
    /// frame will record under, reserving its occurrence slot.
    ///
    /// Both halves are the tracker's: `Salt::resolve` mixes in the
    /// parent so identity follows tree position rather than record
    /// order, and [`SeenIds::resolve`] eagerly disambiguates a salt that
    /// already appeared this frame. Neither is meaningful without the
    /// other — a raw id that skipped disambiguation would collide, and a
    /// disambiguated id that skipped the parent would move with record
    /// order — so they resolve together here rather than being paired up
    /// again by every caller.
    #[inline]
    pub(crate) fn widget_id(&mut self, salt: Salt) -> WidgetId {
        let raw_id = salt.resolve(self.current_parent_id());
        self.ids.resolve(raw_id, salt.is_explicit())
    }

    /// The node `id` was opened under in **this** record pass.
    ///
    /// A direct probe of the id map the pass is filling, so it answers
    /// only after the matching [`Self::open_node`] and panics otherwise
    /// — unlike the cascade lookups on `Ui`, which answer for last
    /// frame. `Scroll` uses it to hand its bar overlay a live handle to
    /// the viewport recorded one line earlier.
    #[inline]
    pub(crate) fn current_node(&self, id: WidgetId) -> NodeId {
        self.ids.curr[&id].node
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

    /// Finalize every tree. Pure structural pass — the surface needed
    /// to evaluate each root's placement is passed to `LayoutEngine::run`.
    /// The paint-anim wake fold is centralised in
    /// [`Self::min_paint_anim_wake`] and run at the tail of
    /// `Ui::frame` for both record + paint-only paths.
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

    /// Minimum `next_wake` across every layer's paint anims, or `None`
    /// when nothing wants a wake — no anims at all, or every one of
    /// them settled. Called from `Ui::frame` after both
    /// record and paint-only paths so the next anim boundary is queued
    /// regardless of which path ran.
    pub(crate) fn min_paint_anim_wake(&self, now: Duration) -> Option<Duration> {
        self.trees
            .iter()
            .flat_map(|tree| &tree.paint_anims.entries)
            .filter_map(|entry| entry.anim.next_wake(now))
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
    /// paint and `None` otherwise. The `Background` is borrowed, not
    /// owned, so it isn't re-copied through the `Widget::record → here →
    /// Tree::open_node → shapes::lower::background` chain on every
    /// chromed widget — see [`Background`]'s own note for why it is not
    /// `Copy`.
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
        if let Some(collision) = self.ids.record_endpoint(widget_id, endpoint) {
            self.report_explicit_collision(collision);
        }
    }

    /// Outlined from [`Self::open_node`]: the `tracing::error!` expansion
    /// reserves stack slots in whatever function it inlines into, taxing
    /// every open with a bigger frame for a path that fires only on a
    /// caller bug.
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

    /// Shared gate for the `add_*` recording entry points: a shape can
    /// only attach to a currently-open node, so widgets can't leak
    /// shapes outside an `open_node` / `close_node` scope.
    fn assert_node_open(&self, layer: Layer, what: &str) {
        debug_assert!(
            !self.scratch[layer].open_frames.is_empty(),
            "{what} called with no open node",
        );
    }

    /// Whether a record pass is in flight — i.e. *some* layer has a node
    /// open.
    ///
    /// `record_pass` opens `WidgetId::VIEWPORT` on `Main` before handing
    /// the `Ui` to the app and closes it after, so this is `true` for
    /// exactly the window in which recording-only entry points are legal.
    ///
    /// Deliberately not `current_layer()`: [`Ui::layer`](crate::Ui::layer) pushes a layer
    /// without opening anything in it, so an overlay scope that has not
    /// recorded a widget yet would read as "not recording" on its own
    /// layer while the frame's record is very much in flight. This is a
    /// frame-level question, not a per-layer one — unlike
    /// [`Self::assert_node_open`], which really is asking about the layer
    /// a shape is about to attach to.
    pub(crate) fn is_recording(&self) -> bool {
        self.scratch.iter().any(|s| !s.open_frames.is_empty())
    }

    /// Lower a user-facing [`Shape`](crate::Shape) (curve flattening, span
    /// stamping, hashing) and append it to the active tree's shape buffer.
    /// Asserts a node is currently open so widgets can't leak shapes
    /// outside an `open_node` / `close_node` scope.
    pub(crate) fn add_shape<S: Lower>(&mut self, shape: S) {
        self.push_shape("add_shape", |tree, store| {
            tree.shapes.add(shape, store).is_some()
        });
    }

    /// Append a `GpuView` shape (a
    /// [`ShapeRecord::Image`](crate::scene::shapes::record::ShapeRecord::Image)
    /// sourced from an
    /// [`ImageSource::GpuView`](crate::scene::shapes::paint::ImageSource::GpuView))
    /// to the active node. Only the redraw `epoch` rides the shape — the
    /// view's `id` + app `paint` live in `Ui::gpu_views` keyed by the
    /// owner's `WidgetId`; this is assembled by `Ui::gpu_view`, not lowered
    /// from a user-facing [`Shape`](crate::Shape), so it skips the lowering
    /// path and can never noop-collapse.
    pub(crate) fn add_gpu_view(&mut self, epoch: u64) {
        self.push_shape("add_gpu_view", |tree, _| {
            tree.shapes.add_gpu_view(epoch);
            true
        });
    }

    /// Same as [`Self::add_shape`], but registers a `PaintAnim` against
    /// the freshly-pushed shape so the encoder applies the sampled
    /// `PaintMod` at paint time and `post_record` folds the anim's
    /// `next_wake` into the host's repaint queue. Drops silently
    /// (no entry pushed) if the shape itself was noop-collapsed.
    /// Effectively invisible shapes stay authored but omit their
    /// animation row until a visible record pass resumes them.
    pub(crate) fn add_shape_animated<S: Lower>(&mut self, shape: S, anim: PaintAnim) {
        let layer = self.current_layer();
        self.assert_node_open(layer, "add_shape_animated");
        // Disjoint borrow: `trees` and `scratch` are separate fields.
        let tree = &mut self.trees[layer];
        let frame = self.scratch[layer]
            .open_frames
            .last_mut()
            .expect("`assert_node_open` above found an open frame");
        let Some(shape_idx) = tree.shapes.add(shape, &self.record_store) else {
            return;
        };
        let row = frame.paint_rows;
        frame.paint_rows += 1;
        if !frame.effectively_visible {
            return;
        }
        tree.paint_anims.push_entry(PaintAnimEntry {
            anim,
            shape_idx,
            row,
            node: frame.node,
        });
    }

    /// Shared body of the plain `add_*` entry points: gate on an open
    /// node, hand `push` the active tree, and charge the open frame one
    /// paint row for whatever it actually stored.
    ///
    /// `push` answers "did this store a paint row" — `false` when the
    /// shape noop-collapsed, so the row counter only advances for shapes
    /// that survived. A bare `bool` because no `add_*` entry point here
    /// wants the record index. `add_shape_animated` does, and it needs
    /// the open frame after the push besides, which would mean handing
    /// the closure the frame too — so it calls `Shapes::add` directly.
    #[inline]
    fn push_shape(&mut self, what: &str, push: impl FnOnce(&mut Tree, &RecordStore) -> bool) {
        let layer = self.current_layer();
        self.assert_node_open(layer, what);
        // Disjoint borrow: record storage, `trees`, and `scratch` are
        // separate fields, so all three can be borrowed for the same call.
        let tree = &mut self.trees[layer];
        if push(tree, &self.record_store) {
            self.scratch[layer]
                .open_frames
                .last_mut()
                .expect("`assert_node_open` above found an open frame")
                .paint_rows += 1;
        }
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
        //
        // Asserted in release: nothing downstream reads the rank again,
        // so a lower-or-equal nest produces no error of its own — the
        // scope paints under the parent it was raised from and overwrites
        // the placement that parent is waiting on, and the frame comes out
        // subtly wrong. That is public-API misuse on a cold path, which is
        // the one case this crate spends a release assert on. `Ui::layer`
        // runs once per side scope, not per node.
        assert!(
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
    fn current_tree(&self) -> &Tree {
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

#[cfg(test)]
pub(crate) mod test_support {
    use crate::primitives::widget_id::WidgetId;
    use crate::scene::forest::Forest;
    use crate::scene::layer::Layer;
    use crate::scene::tree::node_id::NodeId;

    impl Forest {
        /// The node carrying `id` on `layer`. A linear scan — fine for
        /// tests, which is the only caller; the production path reaches
        /// nodes by `NodeId` already.
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
