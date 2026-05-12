//! Per-layer arena collection. The forest holds one [`Tree`] per
//! `Layer` variant; `Ui::layer` switches the active tree, so popup
//! body recording dispatches into a different arena than `Main`
//! and never interleaves.

use crate::forest::element::Element;
use crate::forest::tree::{Layer, NodeId, PendingAnchor, Tree};
use crate::forest::widget_id::WidgetId;
use crate::primitives::size::Size;
use crate::shape::Shape;
use glam::Vec2;
use std::array;
use strum::EnumCount as _;

pub(crate) mod element;
pub(crate) mod node;
pub(crate) mod rollups;
pub(crate) mod shapes;
pub(crate) mod tree;
pub(crate) mod visibility;
pub(crate) mod widget_id;

/// Recording-only state owned by [`Forest`]. The active layer selects
/// which `Tree` receives the next `open_node` / `add_shape`. The
/// anchor for the active scope's next root mint lives on the
/// destination tree's `pending_anchors` stack — pushed by
/// [`Forest::push_layer`], read by `Tree::open_node` on root mint,
/// popped by [`Forest::pop_layer`]. Per-layer ancestor stacks live on
/// each `Tree` itself.
#[derive(Default)]
struct RecordingState {
    /// Active layer for the next `open_node`. `Main` between/outside
    /// `Ui::layer` scopes; switched by `push_scope` / `pop_scope`.
    current_layer: Layer,
    /// Save-stack: one entry per open `push_scope` — the outer layer
    /// is restored on `pop_scope`. Empty outside any layer scope.
    /// Anchors save and restore on the per-`Tree` `pending_anchors`
    /// stack, so nested same-layer pushes (currently forbidden by the
    /// `Forest::push_layer` assert) would also be safe.
    layer_stack: Vec<Layer>,
}

impl RecordingState {
    fn reset(&mut self) {
        self.current_layer = Layer::Main;
        self.layer_stack.clear();
    }

    fn push_scope(&mut self, layer: Layer) {
        self.layer_stack.push(self.current_layer);
        self.current_layer = layer;
    }

    fn pop_scope(&mut self) {
        self.current_layer = self
            .layer_stack
            .pop()
            .expect("pop_scope called without a matching push_scope");
    }
}

/// One arena per [`Layer`]. Recording dispatches `open_node`,
/// `add_shape`, `close_node` to `trees[recording.current_layer as
/// usize]`. Pipeline passes iterate trees via
/// [`Forest::iter_paint_order`].
///
/// **Access convention**: prefer [`Forest::tree`] / [`Forest::tree_mut`]
/// for known-layer access; iterate `trees` directly only for
/// cross-layer aggregation that doesn't care about layer order
/// (e.g. summing record counts).
pub(crate) struct Forest {
    pub(crate) trees: [Tree; Layer::COUNT],
    recording: RecordingState,
}

impl Default for Forest {
    fn default() -> Self {
        Self {
            trees: array::from_fn(|_| Tree::default()),
            recording: RecordingState::default(),
        }
    }
}

impl Forest {
    pub(crate) fn pre_record(&mut self) {
        self.recording.reset();
        for t in &mut self.trees {
            t.pre_record();
        }
    }

    /// Finalize every tree. Pure structural pass — `RootSlot.anchor`
    /// is just a placement; the surface needed to derive each root's
    /// "available" room is passed straight to `LayoutEngine::run`.
    pub(crate) fn post_record(&mut self) {
        assert_eq!(
            self.recording.current_layer,
            Layer::Main,
            "post_record called with active layer {:?} — Ui::layer body forgot to return",
            self.recording.current_layer,
        );
        for layer in Layer::PAINT_ORDER {
            self.trees[layer as usize].post_record();
        }
    }

    pub(crate) fn open_node(&mut self, element: Element) -> NodeId {
        assert!(
            element.id != WidgetId::default(),
            "widget recorded without a `WidgetId` — chain `.id_salt(key)`, \
             `.id(precomputed)`, or `.auto_id()` on the builder before `.show(ui)`. \
             `Foo::new()` no longer derives an id automatically.",
        );
        let layer = self.recording.current_layer;
        self.trees[layer as usize].open_node(element)
    }

    pub(crate) fn close_node(&mut self) {
        let layer = self.recording.current_layer;
        self.trees[layer as usize].close_node();
    }

    /// Lower a user-facing [`Shape`] (curve flattening, span
    /// stamping, hashing) and append it to the active tree's shape
    /// buffer. Asserts a node is currently open so widgets can't leak
    /// shapes outside an `open_node` / `close_node` scope.
    pub(crate) fn add_shape(&mut self, shape: Shape<'_>) {
        let tree = &mut self.trees[self.recording.current_layer as usize];
        assert!(
            !tree.open_frames.is_empty(),
            "add_shape called with no open node",
        );
        tree.shapes.add(shape);
    }

    pub(crate) fn push_layer(&mut self, layer: Layer, anchor: Vec2, size: Option<Size>) {
        assert_eq!(
            self.recording.current_layer,
            Layer::Main,
            "Ui::layer must be called from the Main scope (current: {:?})",
            self.recording.current_layer,
        );
        let tree = &mut self.trees[layer as usize];
        tree.pending_anchors.push(PendingAnchor { anchor, size });
        self.recording.push_scope(layer);
    }

    pub(crate) fn pop_layer(&mut self) {
        let layer = self.recording.current_layer;
        let tree = &mut self.trees[layer as usize];
        assert!(
            tree.open_frames.is_empty(),
            "Ui::layer body left {} node(s) open in layer {:?}",
            tree.open_frames.len(),
            layer,
        );
        tree.pending_anchors
            .pop()
            .expect("pop_layer without matching push_layer");
        self.recording.pop_scope();
    }

    /// Borrow the tree owned by `layer`.
    #[inline]
    pub(crate) fn tree(&self, layer: Layer) -> &Tree {
        &self.trees[layer as usize]
    }

    /// Active recording layer. `Main` outside `Ui::layer` scopes; the
    /// scope's destination layer inside one. Read by widgets that need
    /// to know which arena their record stream is landing in (e.g.
    /// `Grid` / `Scroll` looking up the in-flight node id).
    #[inline]
    pub(crate) fn current_layer(&self) -> Layer {
        self.recording.current_layer
    }

    /// Active recording layer's `Tree::ancestor_disabled`. Read by
    /// `Ui::response_for` to OR inherited-disabled into the response
    /// state without waiting for next-frame cascade.
    pub(crate) fn ancestor_disabled(&self) -> bool {
        self.trees[self.recording.current_layer as usize].ancestor_disabled()
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
