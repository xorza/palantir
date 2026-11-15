//! Every input a cascade run reads, hashed into one value per frame.

use crate::cascade::cascade_source::CascadeSource;
use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::scene::tree::root_slot::RootSlot;
use std::hash::{Hash as _, Hasher as _};

/// What [`CascadeEngine::run`](crate::cascade::engine::CascadeEngine::run) reads. Equal keys skip the run; how much else two keys share picks what the run keeps ([`CascadeReuse`]).
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
    /// Each plane and its `TreeFingerprint::cascade_static`, and each root's requested layer,
    /// which the root rows carry: the structural tables' source.
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
            tree.fingerprint.cascade_static.hash(&mut structure);
            layout[plane].rect_hash().hash(&mut rects);
            // Destructured, so a new root input fails to compile until the key names it.
            for &RootSlot {
                first_node,
                placement: _,
                layer,
            } in &tree.roots
            {
                // The placement reaches the walk only as rects, which `rects` names.
                structure.write_u8(layer.idx() as u8);
                tree.rollups.subtree[first_node.idx()].hash(&mut paint);
            }
            // A root moving between planes is a paint change too.
            paint.write_u8(u8::MAX);
        }
        Self {
            scale: display.scale_factor().to_bits(),
            font_epoch,
            structure: ContentHash::finish(&structure),
            rects: ContentHash::finish(&rects),
            paint: ContentHash::finish(&paint),
        }
    }

    /// Whether a cascade built under `self` keeps its structural tables under `live`: the same
    /// planes, each with the same static fingerprint and its roots in the same layers, whatever
    /// the scale, the fonts and the rows.
    pub(crate) fn same_structure(&self, live: &Self) -> bool {
        self.structure == live.structure
    }

    /// What of a cascade built under `self` stays valid under `live`.
    pub(crate) fn reuse(&self, live: &Self) -> CascadeReuse {
        if !self.same_structure(live) {
            CascadeReuse::Nothing
        } else if self.scale == live.scale && self.font_epoch == live.font_epoch {
            CascadeReuse::Rows
        } else {
            CascadeReuse::Structure
        }
    }
}

/// What a run keeps of the cascade it updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CascadeReuse {
    /// A structural table moved: every table is built again.
    Nothing,
    /// The structural tables hold, but the scale or the fonts moved, which reach rows a node's
    /// own inputs do not name: every node is walked again and each plane's paint arena laid out
    /// afresh.
    Structure,
    /// The structural tables hold, and each node's rows follow from its own inputs: a plane whose
    /// row counts held is patched in place, skipping each node whose inputs held, and the rest
    /// are laid out afresh.
    Rows,
}

#[cfg(test)]
mod tests {
    use crate::cascade::cascade_key::{CascadeKey, CascadeReuse};
    use crate::common::content_hash::ContentHash;
    use crate::common::hash::Hasher;
    use std::hash::Hasher as _;

    /// A hash of the one byte `byte`, so two keys differ in the field it fills alone.
    fn hash(byte: u8) -> ContentHash {
        let mut h = Hasher::new();
        h.write_u8(byte);
        ContentHash::finish(&h)
    }

    /// The structure alone decides whether the tables hold: a moved scale or font epoch keeps
    /// them, with the rows walked again, and a moved structure keeps nothing.
    #[test]
    fn the_structure_alone_decides_whether_the_tables_hold() {
        let built = CascadeKey {
            scale: 1.0f32.to_bits(),
            font_epoch: 0,
            structure: hash(1),
            rects: hash(2),
            paint: hash(3),
        };
        let cases = [
            ("held", built, true, CascadeReuse::Rows),
            (
                "scale moved",
                CascadeKey {
                    scale: 2.0f32.to_bits(),
                    ..built
                },
                true,
                CascadeReuse::Structure,
            ),
            (
                "fonts moved",
                CascadeKey {
                    font_epoch: 1,
                    ..built
                },
                true,
                CascadeReuse::Structure,
            ),
            (
                "rects and paint moved",
                CascadeKey {
                    rects: hash(4),
                    paint: hash(5),
                    ..built
                },
                true,
                CascadeReuse::Rows,
            ),
            (
                "structure moved",
                CascadeKey {
                    structure: hash(6),
                    ..built
                },
                false,
                CascadeReuse::Nothing,
            ),
        ];
        for (label, live, same, reuse) in cases {
            assert_eq!(built.same_structure(&live), same, "{label}");
            assert_eq!(built.reuse(&live), reuse, "{label}");
        }
    }
}
