//! Every input a cascade run reads, hashed into one value per frame.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::display::Display;
use crate::layout::Layout;
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
use crate::scene::per_layer::PerLayer;
use std::hash::Hasher as _;

/// What [`CascadeEngine::run`](crate::cascade::engine::CascadeEngine::run) reads. Equal keys skip the run; keys agreeing on structure refresh geometry and paint in place.
///
/// Sound only while this names every input the walk reads: add new inputs here.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CascadeKey {
    /// `Display::scale_factor` bits, the walk's only display read; surface size arrives via [`LayerKey::rects`].
    scale: u32,
    /// `TextShaper::font_epoch`: a later font load can move ink extent without moving any rect or authoring.
    font_epoch: u32,
    layers: PerLayer<LayerKey>,
}

/// One layer's part of a [`CascadeKey`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct LayerKey {
    /// `TreeFingerprint::cascade_static`: the structural tables' source.
    structure: ContentHash,
    /// `TreeFingerprint::paint_counts`; a moved row count is a full rebuild.
    paint_counts: ContentHash,
    /// `LayerLayout::rect_hash`: while it holds, the walk skips subtrees whose inputs held.
    rects: ContentHash,
    /// Every root's full subtree hash, in order.
    paint: ContentHash,
}

impl CascadeKey {
    pub(crate) fn new(forest: &Forest, layout: &Layout, display: Display, font_epoch: u32) -> Self {
        let mut layers = PerLayer::<LayerKey>::default();
        for (layer, tree) in forest.trees.iter_paint_order() {
            let mut paint = Hasher::new();
            for slot in &tree.roots {
                paint.write_u64(tree.rollups.subtree[slot.first_node.idx()].0);
            }
            layers[layer] = LayerKey {
                structure: tree.fingerprint.cascade_static,
                paint_counts: tree.fingerprint.paint_counts,
                rects: layout[layer].rect_hash(),
                paint: ContentHash(paint.finish()),
            };
        }
        Self {
            scale: display.scale_factor().to_bits(),
            font_epoch,
            layers,
        }
    }

    /// Whether `self`'s structural tables stay valid under `live`, so the rest refreshes in place.
    pub(crate) fn keeps_structure(&self, live: &Self) -> bool {
        self.scale == live.scale
            && self.font_epoch == live.font_epoch
            && self
                .layers
                .iter()
                .zip(live.layers.iter())
                .all(|(a, b)| a.structure == b.structure && a.paint_counts == b.paint_counts)
    }

    /// Whether `layer` arranged every node where it did when `self` was built.
    pub(crate) fn keeps_rects(&self, live: &Self, layer: Layer) -> bool {
        self.layers[layer].rects == live.layers[layer].rects
    }
}
