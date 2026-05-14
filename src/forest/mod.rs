//! Per-layer arena collection. The forest holds one [`Tree`] per
//! `Layer` variant; `Ui::layer` switches the active tree, so popup
//! body recording dispatches into a different arena than `Main`
//! and never interleaves.

use crate::forest::element::Element;
use crate::forest::seen_ids::{RecordOutcome, SeenIds};
use crate::forest::tree::{Layer, NodeId, PendingAnchor, Tree};
use crate::primitives::background::Background;
use crate::primitives::size::Size;
use crate::shape::Shape;
use glam::Vec2;
use std::array;
use strum::EnumCount as _;

/// One explicit-id collision recorded this frame. Both endpoints
/// carry their own `Layer` because the colliding ids can straddle a
/// `push_layer` boundary (e.g. same `.id_salt(...)` in Main and in a
/// Popup body). Resolved at recording time from `SeenIds.curr`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CollisionRecord {
    pub(crate) first: (Layer, NodeId),
    pub(crate) second: (Layer, NodeId),
}

pub(crate) mod element;
pub(crate) mod node;
pub(crate) mod rollups;
pub(crate) mod seen_ids;
pub(crate) mod shapes;
pub(crate) mod tree;
pub(crate) mod visibility;

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

    pub(crate) fn open_node(&mut self, mut element: Element) -> NodeId {
        // `record` runs before `open_node` so a disambiguated
        // `element.id` is what the tree stores — siblings sharing a
        // `widget_id` would corrupt every per-id store. `peek_next_id`
        // hands us the slot `open_node` will fill so `record` can
        // stash it for first-collision lookup.
        let layer = self.current_layer;
        let node = self.trees[layer as usize].peek_next_id();
        let outcome = self.ids.record(&mut element, layer, node);
        let opened = self.trees[layer as usize].open_node(element);
        debug_assert_eq!(opened, node, "Tree::peek_next_id contract violated");
        self.record_collision(outcome, layer, node);
        node
    }

    pub(crate) fn open_node_with_chrome(
        &mut self,
        mut element: Element,
        chrome: Background,
    ) -> NodeId {
        let layer = self.current_layer;
        let node = self.trees[layer as usize].peek_next_id();
        let outcome = self.ids.record(&mut element, layer, node);
        let opened = self.trees[layer as usize].open_node_with_chrome(element, chrome);
        debug_assert_eq!(opened, node, "Tree::peek_next_id contract violated");
        self.record_collision(outcome, layer, node);
        node
    }

    /// Shared between [`Self::open_node`] / [`Self::open_node_with_chrome`].
    /// Logs + records an explicit-id collision (auto collisions are
    /// silent — the disambiguation already ran inside `SeenIds::record`).
    fn record_collision(&mut self, outcome: RecordOutcome, layer: Layer, node: NodeId) {
        if let RecordOutcome::DisambiguatedExplicit { first } = outcome {
            tracing::error!(
                first_layer = ?first.0,
                first_node = ?first.1,
                second_layer = ?layer,
                second_node = ?node,
                "explicit WidgetId collision — disambiguated; per-widget state will not survive between the colliding call sites",
            );
            self.collisions.push(CollisionRecord {
                first,
                second: (layer, node),
            });
        }
    }

    pub(crate) fn close_node(&mut self) {
        self.trees[self.current_layer as usize].close_node();
    }

    /// Lower a user-facing [`Shape`] (curve flattening, span
    /// stamping, hashing) and append it to the active tree's shape
    /// buffer. Asserts a node is currently open so widgets can't leak
    /// shapes outside an `open_node` / `close_node` scope.
    pub(crate) fn add_shape(&mut self, shape: Shape<'_>) {
        let tree = &mut self.trees[self.current_layer as usize];
        assert!(
            !tree.open_frames.is_empty(),
            "add_shape called with no open node",
        );
        tree.shapes.add(shape);
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
