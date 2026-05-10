//! Per-layer arena collection. The forest holds one [`Tree`] per
//! `Layer` variant; `Ui::layer` switches the active tree, so popup
//! body recording dispatches into a different arena than `Main`
//! and never interleaves.

use crate::forest::element::Element;
use crate::forest::seen_ids::SeenIds;
use crate::forest::tree::{Layer, NodeId, Tree};
use crate::forest::widget_id::WidgetId;
use crate::primitives::rect::Rect;
use crate::shape::Shape;
use std::array;
use strum::EnumCount as _;

pub(crate) mod element;
pub(crate) mod node;
pub(crate) mod rollups;
pub(crate) mod seen_ids;
pub(crate) mod tree;
pub(crate) mod visibility;
pub(crate) mod widget_id;

/// Recording-only state owned by [`Forest`]. The active layer selects
/// which `Tree` receives the next `open_node` / `add_shape`. The
/// anchor for the active scope's next root mint lives on the
/// destination tree's `pending_anchor` field — set by
/// [`Forest::push_layer`], consumed by the next `Tree::open_node`
/// that mints a `RootSlot`. Per-layer ancestor stacks live on each
/// `Tree` itself.
#[derive(Default)]
struct RecordingState {
    /// Active layer for the next `open_node`. `Main` between/outside
    /// `Ui::layer` scopes; switched by `push_scope` / `pop_scope`.
    current_layer: Layer,
    /// Save-stack: one entry per open `push_scope` — the outer layer
    /// is restored on `pop_scope`. Empty outside any layer scope.
    /// Anchors don't ride the stack because each `Tree` owns its own
    /// `pending_anchor` and same-layer nesting (which would clobber
    /// it) is forbidden by [`Forest::push_layer`]'s assert.
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
    /// Forest-wide `WidgetId` tracker — collision detection across
    /// all layers, removed-widget diff, and frame rollover. Lives here
    /// (not on `Ui`) so the uniqueness invariant is enforced at the
    /// recording-arena layer instead of by orchestrator convention.
    pub(crate) ids: SeenIds,
}

impl Default for Forest {
    fn default() -> Self {
        Self {
            trees: array::from_fn(|_| Tree::default()),
            recording: RecordingState::default(),
            ids: SeenIds::default(),
        }
    }
}

impl Forest {
    pub(crate) fn begin_frame(&mut self) {
        self.recording.reset();
        self.ids.begin_frame();
        for t in &mut self.trees {
            t.begin_frame();
        }
    }

    /// Finalize every tree. `main_anchor` patches `Main`'s root slots
    /// (their anchor is the surface, only known after recording);
    /// other layers' anchors were stamped at `push_layer` time.
    pub(crate) fn end_frame(&mut self, main_anchor: Rect) {
        assert_eq!(
            self.recording.current_layer,
            Layer::Main,
            "end_frame called with active layer {:?} — Ui::layer body forgot to return",
            self.recording.current_layer,
        );
        for r in &mut self.trees[Layer::Main as usize].roots {
            r.anchor_rect = main_anchor;
        }
        for layer in Layer::PAINT_ORDER {
            self.trees[layer as usize].end_frame();
        }
    }

    pub(crate) fn open_node(&mut self, mut element: Element) -> NodeId {
        // Resolve the widget id at the recording boundary: builders
        // produce an unset id by default and chain `id_salt` /
        // `auto_id` to set it; explicit-id collisions hard-assert in
        // `SeenIds::record`, auto-id collisions get silently
        // disambiguated.
        assert!(
            element.id != WidgetId::default(),
            "widget recorded without a `WidgetId` — chain `.id_salt(key)`, \
             `.id(precomputed)`, or `.auto_id()` on the builder before `.show(ui)`. \
             `Foo::new()` no longer derives an id automatically.",
        );
        element.id = self.ids.record(element.id, element.auto_id);
        let layer = self.recording.current_layer;
        self.trees[layer as usize].open_node(element)
    }

    pub(crate) fn close_node(&mut self) {
        let layer = self.recording.current_layer;
        self.trees[layer as usize].close_node();
    }

    pub(crate) fn add_shape(&mut self, shape: Shape) {
        let layer = self.recording.current_layer;
        self.trees[layer as usize].add_shape(shape);
    }

    pub(crate) fn push_layer(&mut self, layer: Layer, anchor: Rect) {
        assert_eq!(
            self.recording.current_layer,
            Layer::Main,
            "Ui::layer must be called from the Main scope (current: {:?})",
            self.recording.current_layer,
        );
        self.trees[layer as usize].pending_anchor = anchor;
        self.recording.push_scope(layer);
    }

    pub(crate) fn pop_layer(&mut self) {
        let layer = self.recording.current_layer;
        assert!(
            self.trees[layer as usize].open_frames.is_empty(),
            "Ui::layer body left {} node(s) open in layer {:?}",
            self.trees[layer as usize].open_frames.len(),
            layer,
        );
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
