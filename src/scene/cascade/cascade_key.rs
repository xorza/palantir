//! Every input a cascade run reads, hashed into one value per frame.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::display::Display;
use crate::layout::Layout;
use crate::scene::forest::Forest;
use crate::scene::layer::PerLayer;
use std::hash::Hasher as _;

/// What [`CascadeEngine::run`](crate::scene::cascade::engine::CascadeEngine::run)
/// reads, in one place. Two frames with equal keys produce the same cascade,
/// so the run skips. Two frames whose keys agree on everything but paint
/// produce the same non-paint columns, so the run repairs paint in place.
///
/// The skip is sound only while this struct names every input the walk
/// reads. A new cascade input goes here, or the cascade goes stale.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CascadeKey {
    /// `Display::scale_factor`, as bits: the walk's only read of the
    /// display. The surface size reaches the cascade only through the
    /// arranged rects, which [`LayerKey::rects`] holds.
    scale: u32,
    /// `TextShaper::font_epoch`. A face loaded after a run was shaped
    /// can move its ink extent without moving its rect or its
    /// authoring, so nothing per layer sees it.
    font_epoch: u32,
    layers: PerLayer<LayerKey>,
}

/// One layer's part of a [`CascadeKey`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct LayerKey {
    /// `TreeFingerprint::cascade_static`: identity, nesting and the
    /// layout half of every node.
    structure: ContentHash,
    /// `TreeFingerprint::paint_counts`. The incremental walk repairs paint
    /// rows in place, so a row count that moved is a full rebuild.
    paint_counts: ContentHash,
    /// `LayerLayout::rect_hash`: the arranged geometry.
    rects: ContentHash,
    /// Every root's full subtree hash, in order — the paint half the
    /// incremental walk repairs.
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

    /// Whether a cascade built from `self` keeps every non-paint column
    /// valid under `live`: only paint differs, which the incremental walk
    /// repairs.
    pub(crate) fn differs_only_in_paint(&self, live: &Self) -> bool {
        self.scale == live.scale
            && self.font_epoch == live.font_epoch
            && self.layers.iter().zip(live.layers.iter()).all(|(a, b)| {
                a.structure == b.structure && a.paint_counts == b.paint_counts && a.rects == b.rects
            })
    }
}
