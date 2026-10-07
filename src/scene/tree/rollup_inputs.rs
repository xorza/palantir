//! What [`Tree::compute_rollups`](crate::scene::tree::Tree) read on the previous pass, so an identical pass keeps them.

use crate::common::content_hash::ContentHash;
use crate::layout::drivers::scrollbars::scrollbars_def::ResolvedScrollbarsDef;
use crate::primitives::layout::track::{GridDef, Track};
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::panel_extras::PanelExtras;
use crate::scene::tree::Tree;
use crate::scene::tree::node_record::NodeRecord;
#[cfg(debug_assertions)]
use crate::scene::tree::subtree_rollups::SubtreeRollups;
use crate::shape::paint::chrome_row::ChromeRow;
#[cfg(debug_assertions)]
use fixedbitset::FixedBitSet;
use soa_rs::Soa;
use std::mem;

/// The last pass's recorded columns, swapped out of the tree at `Tree::pre_record` (not cleared) so the buffers
/// trade places and neither allocates once warm. Each column is compared as the rollups read it (shapes and chrome
/// by hash, the rest field by field), so equal inputs give equal rollups.
#[derive(Debug, Default)]
pub(super) struct RollupInputs {
    records: Soa<NodeRecord>,
    bounds_table: Vec<BoundsExtras>,
    panel_table: Vec<PanelExtras>,
    chrome_table: Vec<ChromeRow>,
    shape_hashes: Vec<ContentHash>,
    grid_tracks: Vec<Track>,
    grid_defs: Vec<GridDef>,
    scrollbar_defs: Vec<ResolvedScrollbarsDef>,
    rolled: bool,
    /// Whether the tree's rollups were computed from the columns it holds now; set by `Tree::post_record` and handed
    /// to [`Self::rolled`] by the next [`Self::take_from`], so a pass that never reached it leaves nothing to keep.
    current_rolled: bool,
}

impl RollupInputs {
    /// Takes `tree`'s recorded columns as the last pass's, leaving it the emptied buffers of the pass before.
    pub(super) fn take_from(&mut self, tree: &mut Tree) {
        mem::swap(&mut self.records, &mut tree.records);
        mem::swap(&mut self.bounds_table, &mut tree.bounds_table);
        mem::swap(&mut self.panel_table, &mut tree.panel_table);
        mem::swap(&mut self.chrome_table, &mut tree.chrome_table);
        mem::swap(&mut self.shape_hashes, &mut tree.shapes.hashes);
        mem::swap(&mut self.grid_tracks, &mut tree.grid_tracks);
        mem::swap(&mut self.grid_defs, &mut tree.grid_defs);
        mem::swap(&mut self.scrollbar_defs, &mut tree.scrollbar_defs);
        tree.records.clear();
        tree.bounds_table.clear();
        tree.panel_table.clear();
        tree.chrome_table.clear();
        tree.shapes.hashes.clear();
        tree.grid_tracks.clear();
        tree.grid_defs.clear();
        tree.scrollbar_defs.clear();
        self.rolled = mem::replace(&mut self.current_rolled, false);
    }

    pub(super) const fn note_rolled(&mut self) {
        self.current_rolled = true;
    }

    pub(super) fn hold_for(&self, tree: &Tree) -> bool {
        if !self.rolled {
            return false;
        }
        let (now, last) = (&tree.records, &self.records);
        // Small columns a frame most often changes come first, so a moved pass stops before the node columns.
        now.len() == last.len()
            && same_bytes(&tree.shapes.hashes, &self.shape_hashes)
            && all_alike(&tree.panel_table, &self.panel_table, PartialEq::eq)
            && all_alike(&tree.chrome_table, &self.chrome_table, |a, b| {
                a.hash == b.hash
            })
            && all_alike(&tree.bounds_table, &self.bounds_table, PartialEq::eq)
            && all_alike(&tree.grid_tracks, &self.grid_tracks, PartialEq::eq)
            && all_alike(&tree.grid_defs, &self.grid_defs, PartialEq::eq)
            && all_alike(&tree.scrollbar_defs, &self.scrollbar_defs, PartialEq::eq)
            && same_bytes(now.widget_id(), last.widget_id())
            && same_bytes(now.subtree_end(), last.subtree_end())
            && same_bytes(now.shape_span(), last.shape_span())
            && same_bytes(now.attrs(), last.attrs())
            && same_bytes(now.extras(), last.extras())
            && same_bytes(now.layout(), last.layout())
    }
}

fn same_bytes<T: bytemuck::NoUninit>(now: &[T], last: &[T]) -> bool {
    bytemuck::cast_slice::<T, u8>(now) == bytemuck::cast_slice::<T, u8>(last)
}

/// Whether `alike` holds for every pair, folded branch-free so runs compare as vectors; stops at the first differing chunk.
fn all_alike<T>(now: &[T], last: &[T], alike: impl Fn(&T, &T) -> bool) -> bool {
    const CHUNK: usize = 64;
    now.len() == last.len()
        && now
            .chunks(CHUNK)
            .zip(last.chunks(CHUNK))
            .all(|(a, b)| a.iter().zip(b).fold(true, |all, (x, y)| all & alike(x, y)))
}

/// Where `Tree::assert_rollups_hold` keeps the rollups a pass kept while recomputing them.
#[cfg(debug_assertions)]
#[derive(Debug, Default)]
pub(super) struct RollupCheck {
    pub(super) rollups: SubtreeRollups,
    pub(super) container_text: FixedBitSet,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two empty passes record equal columns, so only the pairing decides: rollups hold once `post_record` noted them.
    #[test]
    fn rollups_hold_only_for_a_pass_that_rolled() {
        let mut tree = Tree::default();
        let mut inputs = RollupInputs::default();
        inputs.take_from(&mut tree);
        assert!(!inputs.hold_for(&tree), "nothing rolled yet");
        inputs.note_rolled();
        inputs.take_from(&mut tree);
        assert!(inputs.hold_for(&tree), "the last pass rolled");
        inputs.take_from(&mut tree);
        assert!(!inputs.hold_for(&tree), "the last pass never rolled");
    }
}
