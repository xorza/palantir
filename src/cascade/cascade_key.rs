//! Every input a cascade run reads, hashed into one value per frame.

use crate::cascade::cascade_source::CascadeSource;
use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use std::hash::Hasher as _;

/// What [`CascadeEngine::run`](crate::cascade::engine::CascadeEngine::run) reads. Equal keys skip the run; keys agreeing on structure refresh geometry and paint in place.
///
/// Folds every plane in paint order, so a key stays a fixed size however deep overlays nest; each plane's own rect hash, which the incremental walk compares plane by plane, is kept on its [`PlaneCascade`](crate::cascade::plane_cascade::PlaneCascade).
///
/// Sound only while this names every input the walk reads: add new inputs here.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CascadeKey {
    /// `Display::scale_factor` bits, the walk's only display read; surface size arrives via [`Self::rects`].
    scale: u32,
    /// [`CascadeSource::font_epoch`].
    font_epoch: u32,
    /// Each plane's `TreeFingerprint::cascade_static` and `TreeFingerprint::paint_counts`: the structural tables' source; a moved row count is a full rebuild.
    structure: ContentHash,
    /// Each plane's `PlaneLayout::rect_hash`.
    rects: ContentHash,
    /// Every root's full subtree hash, in order.
    paint: ContentHash,
}

impl CascadeKey {
    pub(crate) fn new(source: CascadeSource<'_>) -> Self {
        let CascadeSource {
            forest,
            layout,
            display,
            font_epoch,
        } = source;
        let mut structure = Hasher::new();
        let mut rects = Hasher::new();
        let mut paint = Hasher::new();
        for (plane, tree) in forest.trees.iter_paint_order() {
            structure.write_u8(plane.layer.idx() as u8);
            structure.write_u8(plane.depth);
            structure.write_u64(tree.fingerprint.cascade_static.0);
            structure.write_u64(tree.fingerprint.paint_counts.0);
            rects.write_u64(layout[plane].rect_hash().0);
            for slot in &tree.roots {
                paint.write_u64(tree.rollups.subtree[slot.first_node.idx()].0);
            }
            // A root moving between planes is a paint change too.
            paint.write_u8(u8::MAX);
        }
        Self {
            scale: display.scale_factor().to_bits(),
            font_epoch,
            structure: ContentHash(structure.finish()),
            rects: ContentHash(rects.finish()),
            paint: ContentHash(paint.finish()),
        }
    }

    /// Whether `self`'s structural tables stay valid under `live`, so the rest refreshes in place.
    pub(crate) fn keeps_structure(&self, live: &Self) -> bool {
        self.scale == live.scale
            && self.font_epoch == live.font_epoch
            && self.structure == live.structure
    }
}
