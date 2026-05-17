//! Per-layer arena collection. The forest holds one [`Tree`] per
//! `Layer` variant; `Ui::layer` switches the active tree, so popup
//! body recording dispatches into a different arena than `Main`
//! and never interleaves.

use crate::animation::paint::PaintAnim;
use crate::common::frame_arena::FrameArena;
use crate::forest::element::Element;
use crate::forest::seen_ids::{Endpoint, RecordOutcome, SeenIds};
use crate::forest::tree::paint_anims::PaintAnimEntry;
use crate::forest::tree::{NodeId, PendingAnchor, Tree};
use crate::primitives::background::Background;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::gradient_atlas::GradientAtlas;
use crate::shape::Shape;
use glam::Vec2;
use std::array;
use std::time::Duration;
use strum::EnumCount as _;

/// One explicit-id collision recorded this frame. Both endpoints
/// carry their own `Layer` because the colliding ids can straddle a
/// `push_layer` boundary (e.g. same `.id_salt(...)` in Main and in a
/// Popup body). Resolved at recording time from `SeenIds.curr`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CollisionRecord {
    pub(crate) first: Endpoint,
    pub(crate) second: Endpoint,
}

pub(crate) mod element;
pub(crate) mod node;
pub(crate) mod rollups;
pub(crate) mod seen_ids;
pub(crate) mod shapes;
pub mod tree;
pub(crate) mod visibility;

/// Paint / hit-test order across layers. Lower variants paint first
/// (under) and hit-test last (under). Total order — popups beat the
/// main tree, modals beat popups, tooltips beat modals, debug beats
/// everything. See `docs/popups.md`.
///
/// `#[repr(u8)]` + the contiguous variant layout means `layer as usize`
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
}

/// One arena per [`Layer`]. Recording dispatches `open_node`,
/// `add_shape`, `close_node` to `trees[current_layer as usize]`.
/// Pipeline passes iterate trees via [`Forest::iter_paint_order`].
///
/// **Access convention**: prefer [`Forest::tree`] / [`Forest::tree_mut`]
/// for known-layer access; iterate `trees` directly only for
/// cross-layer aggregation that doesn't care about layer order
/// (e.g. summing record counts).
pub(crate) struct Forest {
    pub(crate) trees: [Tree; Layer::COUNT],
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
    /// Active layer for the next `open_node`. `Main` between/outside
    /// `Ui::layer` scopes; switched by `push_layer` / `pop_layer`.
    pub(crate) current_layer: Layer,
    /// Save-stack: one entry per open `push_layer` — the outer layer
    /// is restored on `pop_layer`. Empty outside any layer scope.
    /// Anchors save and restore on the per-`Tree` `pending_anchors`
    /// stack, so nested same-layer pushes (currently forbidden by the
    /// `Forest::push_layer` assert) would also be safe.
    layer_stack: Vec<Layer>,
}

impl Default for Forest {
    fn default() -> Self {
        Self {
            trees: array::from_fn(|_| Tree::default()),
            ids: SeenIds::default(),
            collisions: Vec::new(),
            current_layer: Layer::Main,
            layer_stack: Vec::new(),
        }
    }
}

impl Forest {
    pub(crate) fn pre_record(&mut self) {
        self.current_layer = Layer::Main;
        self.layer_stack.clear();
        self.ids.pre_record();
        self.collisions.clear();
        for t in &mut self.trees {
            t.pre_record();
        }
    }

    /// Finalize every tree. Pure structural pass — `RootSlot.anchor`
    /// is just a placement; the surface needed to derive each root's
    /// "available" room is passed straight to `LayoutEngine::run`.
    /// The paint-anim wake fold is centralised in
    /// [`Self::min_paint_anim_wake`] and run at the tail of
    /// `Ui::frame_inner` for both record + paint-only paths.
    #[profiling::function]
    pub(crate) fn post_record(&mut self) {
        assert_eq!(
            self.current_layer,
            Layer::Main,
            "post_record called with active layer {:?} — Ui::layer body forgot to return",
            self.current_layer,
        );
        for layer in Layer::PAINT_ORDER {
            self.trees[layer as usize].post_record();
        }
    }

    /// Minimum `next_wake` across every layer's paint anims, or
    /// `Duration::MAX` when nothing wants a wake. Called from
    /// `Ui::frame_inner` after both record and paint-only paths so the
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

    /// Open a node whose id has already been resolved from
    /// `element.salt` upstream (typically by [`crate::Ui::node`] via
    /// [`crate::Ui::make_persistent_id`]). Returns the `WidgetId`
    /// actually written into the tree — equal to `widget_id`
    /// except on a sibling collision, where the occurrence-counter
    /// disambiguation in [`SeenIds::record`] kicks in.
    ///
    /// `chrome` is `Some((bg, arena, atlas))` for nodes with a
    /// background paint and `None` otherwise.
    #[inline]
    pub(crate) fn open_node(
        &mut self,
        widget_id: WidgetId,
        element: Element,
        chrome: Option<(Background, &FrameArena, &GradientAtlas)>,
    ) -> WidgetId {
        let layer = self.current_layer;
        let is_explicit = element.salt.is_explicit();
        let node = self.current_tree_mut().peek_next_id();
        let (final_id, outcome) = self.ids.record(widget_id, is_explicit, layer, node);
        let opened = self.current_tree_mut().open_node(final_id, element, chrome);
        debug_assert_eq!(opened, node, "Tree::peek_next_id contract violated");
        self.record_collision(outcome, layer, node);
        final_id
    }

    /// Shared between [`Self::open_node`] / [`Self::open_node_with_chrome`].
    /// Logs + records an explicit-id collision (auto collisions are
    /// silent — the disambiguation already ran inside `SeenIds::record`).
    fn record_collision(&mut self, outcome: RecordOutcome, layer: Layer, node: NodeId) {
        if let RecordOutcome::DisambiguatedExplicit { first } = outcome {
            let second = Endpoint { layer, node };
            tracing::error!(
                first_layer = ?first.layer,
                first_node = ?first.node,
                second_layer = ?second.layer,
                second_node = ?second.node,
                "explicit WidgetId collision — disambiguated; per-widget state will not survive between the colliding call sites",
            );
            self.collisions.push(CollisionRecord { first, second });
        }
    }

    pub(crate) fn close_node(&mut self) {
        self.current_tree_mut().close_node();
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
        let tree = self.current_tree_mut();
        assert!(
            !tree.open_frames.is_empty(),
            "add_shape called with no open node",
        );
        if tree.shapes.add(shape, arena, atlas).is_some() {
            tree.paint_anims.push_unanimated();
        }
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
        let tree = self.current_tree_mut();
        assert!(
            !tree.open_frames.is_empty(),
            "add_shape_animated called with no open node",
        );
        let Some(shape_idx) = tree.shapes.add(shape, arena, atlas) else {
            return;
        };
        tree.paint_anims
            .push_entry(PaintAnimEntry { anim, shape_idx });
    }

    pub(crate) fn push_layer(&mut self, layer: Layer, anchor: Vec2, size: Option<Size>) {
        assert_eq!(
            self.current_layer,
            Layer::Main,
            "Ui::layer must be called from the Main scope (current: {:?})",
            self.current_layer,
        );
        let tree = &mut self.trees[layer as usize];
        assert!(
            tree.open_frames.is_empty(),
            "Ui::layer({:?}) called while a node is still open in that layer",
            layer,
        );
        tree.pending_anchors.push(PendingAnchor { anchor, size });
        self.layer_stack.push(self.current_layer);
        self.current_layer = layer;
    }

    pub(crate) fn pop_layer(&mut self) {
        let layer = self.current_layer;
        let tree = &mut self.trees[layer as usize];
        assert!(
            tree.open_frames.is_empty(),
            "Ui::layer body left {} node(s) open in layer {:?}",
            tree.open_frames.len(),
            layer,
        );
        tree.pending_anchors.pop();
        self.current_layer = self
            .layer_stack
            .pop()
            .expect("pop_layer without matching push_layer");
    }

    /// Borrow the tree owned by `layer`.
    #[inline]
    pub(crate) fn tree(&self, layer: Layer) -> &Tree {
        &self.trees[layer as usize]
    }

    /// Mutably borrow the tree owned by `layer`.
    #[inline]
    pub(crate) fn tree_mut(&mut self, layer: Layer) -> &mut Tree {
        &mut self.trees[layer as usize]
    }

    /// Borrow the tree for the [`Self::current_layer`] — the one
    /// `open_node` / `add_shape` dispatch to. Convenience over
    /// `tree(current_layer)` for the very common case.
    #[inline]
    pub(crate) fn current_tree(&self) -> &Tree {
        &self.trees[self.current_layer as usize]
    }

    /// Mutably borrow the tree for the [`Self::current_layer`].
    #[inline]
    pub(crate) fn current_tree_mut(&mut self) -> &mut Tree {
        &mut self.trees[self.current_layer as usize]
    }

    /// Iterate trees in paint order (`Layer::PAINT_ORDER`), pairing
    /// each with its layer tag. Pipeline passes consume this to
    /// process layers bottom-up.
    pub(crate) fn iter_paint_order(&self) -> impl Iterator<Item = (Layer, &Tree)> {
        Layer::PAINT_ORDER
            .iter()
            .copied()
            .map(move |layer| (layer, &self.trees[layer as usize]))
    }
}
