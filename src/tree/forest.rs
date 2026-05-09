//! Per-layer arena collection. The forest holds one [`Tree`] per
//! `Layer` variant; `Ui::layer` switches the active tree, so popup
//! body recording dispatches into a different arena than `Main`
//! and never interleaves.

use crate::primitives::background::Background;
use crate::primitives::rect::Rect;
use crate::shape::Shape;
use crate::tree::element::Element;
use crate::tree::recording::RecordingState;
use crate::tree::seen_ids::SeenIds;
use crate::tree::{Layer, NodeId, Tree};
use std::array;
use strum::EnumCount as _;

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
    pub(crate) recording: RecordingState,
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

    pub(crate) fn open_node(&mut self, element: Element, chrome: Option<Background>) -> NodeId {
        let layer = self.recording.current_layer;
        self.trees[layer as usize].open_node(element, chrome)
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
