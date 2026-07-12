//! Per-layer arena collection. The forest holds one [`Tree`] per
//! `Layer` variant; `Ui::layer` switches the active tree, so popup
//! body recording dispatches into a different arena than `Main`
//! and never interleaves.

use crate::forest::element::Element;
use crate::forest::frame_arena::FrameArena;
use crate::forest::per_layer::PerLayer;
use crate::forest::seen_ids::{Endpoint, EndpointOutcome, SeenIds};
use crate::forest::tree::Tree;
use crate::forest::tree::paint_anims::{PaintAnim, PaintAnimEntry};
use crate::forest::tree::record::{Placement, RecordingScratch};
use crate::primitives::background::Background;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::gradient_atlas::GradientAtlas;
use crate::shape::Shape;
use glam::Vec2;
use std::time::Duration;

/// One explicit-id collision recorded this frame. Both endpoints
/// carry their own `Layer` because the colliding ids can straddle a
/// `push_layer` boundary (e.g. same `.id_salt(...)` in Main and in a
/// Popup body). Resolved at recording time from `SeenIds.curr`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CollisionRecord {
    pub(crate) first: Endpoint,
    pub(crate) second: Endpoint,
}

/// Background paint inputs for a chromed node, threaded by reference
/// from `Ui::node` through [`Forest::open_node`] to `Tree::open_node`
/// so the 168 B `Background` isn't copied on every chromed widget every
/// frame. `None` chrome means no background paint.
#[derive(Clone, Copy)]
pub(crate) struct Chrome<'a> {
    pub(crate) bg: &'a Background,
    pub(crate) arena: &'a FrameArena,
    pub(crate) atlas: &'a GradientAtlas,
}

pub(crate) mod element;
pub(crate) mod frame_arena;
pub(crate) mod node;
pub(crate) mod per_layer;
pub(crate) mod rollups;
pub(crate) mod seen_ids;
pub(crate) mod shapes;
pub(crate) mod tree;
pub(crate) mod visibility;

/// Paint / hit-test order across layers. Lower variants paint first
/// (under) and hit-test last (under). Total order — popups beat the
/// main tree, modals beat popups, tooltips beat modals, debug beats
/// everything. See `docs/popups.md`.
///
/// `#[repr(u8)]` + the contiguous variant layout means `layer.idx()`
/// is a valid index into `[T; Layer::COUNT]` per-layer storage. With
/// the forest topology each variant owns its own [`tree::Tree`] arena.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, strum::EnumCount)]
pub enum Layer {
    #[default]
    Main = 0,
    Popup = 1,
    Modal = 2,
    Tooltip = 3,
    Debug = 4,
}

impl Layer {
    /// Paint order (low → high). Iterate trees in this order so layers
    /// paint bottom-up; reverse for topmost-first hit-test traversal.
    pub(crate) const PAINT_ORDER: [Layer; <Layer as strum::EnumCount>::COUNT] = [
        Layer::Main,
        Layer::Popup,
        Layer::Modal,
        Layer::Tooltip,
        Layer::Debug,
    ];

    /// Discriminant as a `usize` — the canonical key into per-layer
    /// `[T; Layer::COUNT]` arrays. Repeats the `repr(u8)` discriminant
    /// in `usize` form so call sites don't sprinkle `as usize` casts
    /// (each of which reads like a fallible narrowing even though
    /// it's branchless).
    #[inline]
    pub(crate) const fn idx(self) -> usize {
        self as usize
    }
}

/// One arena per [`Layer`]. Recording dispatches `open_node`,
/// `add_shape`, `close_node` to `trees[current_layer.idx()]`.
/// Pipeline passes iterate trees via [`Forest::iter_paint_order`].
///
/// **Access convention**: prefer [`Forest::tree`] / [`Forest::tree_mut`]
/// for known-layer access; iterate `trees` directly only for
/// cross-layer aggregation that doesn't care about layer order
/// (e.g. summing record counts).
#[derive(Default)]
pub(crate) struct Forest {
    pub(crate) trees: PerLayer<Tree>,
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
    /// `Ui::node` — gets the same collision check.
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
    /// per-`Tree` `pending_anchor` slot single-occupancy. Retained across
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

    /// Finalize every tree. Pure structural pass — `RootSlot.placement.anchor`
    /// is just a placement; the surface needed to derive each root's
    /// "available" room is passed straight to `LayoutEngine::run`.
    /// The paint-anim wake fold is centralised in
    /// [`Self::min_paint_anim_wake`] and run at the tail of
    /// `Ui::frame` for both record + paint-only paths.
    #[profiling::function]
    pub(crate) fn post_record(&mut self) {
        let active = self.current_layer();
        assert_eq!(
            active,
            Layer::Main,
            "post_record called with active layer {active:?} — Ui::layer body forgot to return",
        );
        for layer in Layer::PAINT_ORDER {
            let scratch = &self.scratch[layer];
            assert!(
                scratch.open_frames.is_empty(),
                "post_record: layer {layer:?} has {} node(s) still open — a widget builder forgot close_node",
                scratch.open_frames.len(),
            );
            self.trees[layer].post_record();
        }
    }

    /// Minimum `next_wake` across every layer's paint anims, or
    /// `Duration::MAX` when nothing wants a wake. Called from
    /// `Ui::frame` after both record and paint-only paths so the
    /// next anim boundary is queued regardless of which path ran.
    pub(crate) fn min_paint_anim_wake(&self, now: Duration) -> Duration {
        let mut min_wake = Duration::MAX;
        for tree in &self.trees {
            for entry in &tree.paint_anims.entries {
                let w = entry.anim.next_wake(now);
                if w < min_wake {
                    min_wake = w;
                }
            }
        }
        min_wake
    }

    /// Open a node whose id has already been resolved + disambiguated
    /// upstream by [`crate::Ui::widget_id`] (which calls
    /// `SeenIds::resolve` eagerly so the returned id matches what the
    /// tree, cascade, and `response_for` see). This function takes
    /// `widget_id` verbatim, records the endpoint via
    /// `SeenIds::record_endpoint` (also emitting any pending explicit
    /// collision pair), and opens the node in the active tree.
    ///
    /// `chrome` is `Some(Chrome { .. })` for nodes with a background
    /// paint and `None` otherwise. The `Background` is borrowed (not
    /// owned) so its 168 B don't get copied through the
    /// `Ui::node → here → Tree::open_node →
    /// FrameArena::lower_background` chain on every chromed widget.
    #[inline]
    pub(crate) fn open_node(
        &mut self,
        widget_id: WidgetId,
        element: Element,
        chrome: Option<Chrome<'_>>,
    ) {
        let layer = self.current_layer();
        let node = self.trees[layer].peek_next_id();
        let endpoint = Endpoint { layer, node };
        if let EndpointOutcome::ExplicitCollision { first, second } =
            self.ids.record_endpoint(widget_id, endpoint)
        {
            self.report_explicit_collision(first, second);
        }
        // Disjoint borrow: `trees` and `scratch` are separate fields
        // on `Forest`, so both can be mutably borrowed at the same call.
        let tree = &mut self.trees[layer];
        let scratch = &mut self.scratch[layer];
        tree.open_node(scratch, node, widget_id, element, chrome);
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

    /// Lower a user-facing [`Shape`] (curve flattening, span
    /// stamping, hashing) and append it to the active tree's shape
    /// buffer. Asserts a node is currently open so widgets can't leak
    /// shapes outside an `open_node` / `close_node` scope.
    pub(crate) fn add_shape(
        &mut self,
        shape: Shape<'_>,
        arena: &FrameArena,
        atlas: &GradientAtlas,
    ) {
        let layer = self.current_layer();
        assert!(
            !self.scratch[layer].open_frames.is_empty(),
            "add_shape called with no open node",
        );
        // No `paint_anims.by_shape` bookkeeping on the unanimated path —
        // `PaintAnims` lazily grows the column only when a real anim
        // shows up. Saves one `Vec::push` per shape every frame.
        let _ = self.trees[layer].shapes.add(shape, arena, atlas);
    }

    /// Append a `GpuView` shape (a [`ShapeRecord::GpuView`]) to the active
    /// node. Only the redraw `epoch` rides the shape — the view's `id` + app
    /// `paint` live in `Ui::gpu_views` keyed by the owner's `WidgetId`; this is
    /// assembled by `Ui::gpu_view`, not lowered from a user-facing [`Shape`],
    /// so it skips the lowering path.
    pub(crate) fn add_gpu_view(&mut self, epoch: u64) {
        let layer = self.current_layer();
        assert!(
            !self.scratch[layer].open_frames.is_empty(),
            "add_gpu_view called with no open node",
        );
        self.trees[layer].shapes.add_gpu_view(epoch);
    }

    /// Same as `add_shape`, but registers a `PaintAnim` against the
    /// freshly-pushed shape so the encoder applies the sampled
    /// `PaintMod` at paint time and `post_record` folds the anim's
    /// `next_wake` into the host's repaint queue. Drops silently
    /// (no entry pushed) if the shape itself was noop-collapsed.
    pub(crate) fn add_shape_animated(
        &mut self,
        shape: Shape<'_>,
        anim: PaintAnim,
        arena: &FrameArena,
        atlas: &GradientAtlas,
    ) {
        let layer = self.current_layer();
        assert!(
            !self.scratch[layer].open_frames.is_empty(),
            "add_shape_animated called with no open node",
        );
        let node_idx = self.scratch[layer].open_frames.last().unwrap().node.0;
        let tree = &mut self.trees[layer];
        let Some(shape_idx) = tree.shapes.add(shape, arena, atlas) else {
            return;
        };
        tree.paint_anims.push_entry(PaintAnimEntry {
            anim,
            shape_idx,
            node_idx,
        });
    }

    pub(crate) fn push_layer(&mut self, layer: Layer, anchor: Vec2, size: Option<Size>) {
        let active = self.current_layer();
        // A nested side layer must paint *above* the scope it's raised
        // from. The cross-layer scheme has no per-node z-index — paint
        // and hit order are entirely `Layer::PAINT_ORDER` — so `layer`
        // must rank strictly higher than the active scope. This admits
        // the real cases (a tooltip rising from a popup or modal body:
        // Tooltip > Popup, Tooltip > Modal) and rejects a lower-or-equal
        // nest, which would record fine but then render *underneath* its
        // parent (occluded, un-hittable). Equal is rejected too: it would
        // also clobber the single per-layer `pending_anchor` slot.
        // Strictly increasing ⇒ each layer appears at most once on the
        // stack, so that slot stays single-occupancy without a guard.
        assert!(
            layer > active,
            "Ui::layer({layer:?}) must rank above the current scope ({active:?}) \
             in Layer::PAINT_ORDER — a nested layer painting under its parent is a bug",
        );
        let scratch = &mut self.scratch[layer];
        assert!(
            scratch.open_frames.is_empty(),
            "Ui::layer({layer:?}) called while a node is still open in that layer",
        );
        scratch.pending_anchor = Some(Placement { anchor, size });
        self.layer_stack.push(layer);
    }

    pub(crate) fn pop_layer(&mut self) {
        let layer = self
            .layer_stack
            .pop()
            .expect("pop_layer without matching push_layer");
        let scratch = &mut self.scratch[layer];
        assert!(
            scratch.open_frames.is_empty(),
            "Ui::layer body left {} node(s) open in layer {:?}",
            scratch.open_frames.len(),
            layer,
        );
        scratch.pending_anchor = None;
    }

    /// Borrow the tree owned by `layer`.
    #[inline]
    pub(crate) fn tree(&self, layer: Layer) -> &Tree {
        &self.trees[layer]
    }

    /// Mutably borrow the tree owned by `layer`.
    #[inline]
    pub(crate) fn tree_mut(&mut self, layer: Layer) -> &mut Tree {
        &mut self.trees[layer]
    }

    /// Borrow the tree for the [`Self::current_layer`] — the one
    /// `open_node` / `add_shape` dispatch to. Convenience over
    /// `tree(current_layer())` for the very common case.
    #[inline]
    pub(crate) fn current_tree(&self) -> &Tree {
        &self.trees[self.current_layer()]
    }

    /// Recording-only scratch for the active layer. Read by
    /// `Ui::widget_id` (parent lookup) and the disabled
    /// cascade at record time.
    #[inline]
    pub(crate) fn current_scratch(&self) -> &RecordingScratch {
        &self.scratch[self.current_layer()]
    }

    /// Iterate trees in paint order (`Layer::PAINT_ORDER`), pairing
    /// each with its layer tag. Pipeline passes consume this to
    /// process layers bottom-up.
    pub(crate) fn iter_paint_order(&self) -> impl Iterator<Item = (Layer, &Tree)> {
        self.trees.iter_paint_order()
    }
}
