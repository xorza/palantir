//! What [`Tree::compute_rollups`](crate::scene::tree::Tree) read on the previous pass, so an identical pass keeps them.

use crate::common::content_hash::ContentHash;
use crate::primitives::layout::grid_def::GridDef;
use crate::primitives::layout::track::Track;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::panel_extras::PanelExtras;
use crate::scene::scrollbars_def::ResolvedScrollbarsDef;
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
    cur_rolled: bool,
}

/// A tree's recorded columns the rollups read, which [`RollupInputs::take_from`] trades for the
/// last pass's.
#[derive(Debug)]
pub(super) struct RecordedColumns<'a> {
    pub(super) records: &'a mut Soa<NodeRecord>,
    pub(super) bounds_table: &'a mut Vec<BoundsExtras>,
    pub(super) panel_table: &'a mut Vec<PanelExtras>,
    pub(super) chrome_table: &'a mut Vec<ChromeRow>,
    pub(super) shape_hashes: &'a mut Vec<ContentHash>,
    pub(super) grid_tracks: &'a mut Vec<Track>,
    pub(super) grid_defs: &'a mut Vec<GridDef>,
    pub(super) scrollbar_defs: &'a mut Vec<ResolvedScrollbarsDef>,
}

impl RollupInputs {
    /// Takes `tree`'s recorded columns as the last pass's, leaving it the emptied buffers of the pass before.
    pub(super) fn take_from(&mut self, tree: RecordedColumns<'_>) {
        let RecordedColumns {
            records,
            bounds_table,
            panel_table,
            chrome_table,
            shape_hashes,
            grid_tracks,
            grid_defs,
            scrollbar_defs,
        } = tree;
        mem::swap(&mut self.records, records);
        mem::swap(&mut self.bounds_table, bounds_table);
        mem::swap(&mut self.panel_table, panel_table);
        mem::swap(&mut self.chrome_table, chrome_table);
        mem::swap(&mut self.shape_hashes, shape_hashes);
        mem::swap(&mut self.grid_tracks, grid_tracks);
        mem::swap(&mut self.grid_defs, grid_defs);
        mem::swap(&mut self.scrollbar_defs, scrollbar_defs);
        records.clear();
        bounds_table.clear();
        panel_table.clear();
        chrome_table.clear();
        shape_hashes.clear();
        grid_tracks.clear();
        grid_defs.clear();
        scrollbar_defs.clear();
        self.rolled = mem::replace(&mut self.cur_rolled, false);
    }

    pub(super) const fn note_rolled(&mut self) {
        self.cur_rolled = true;
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
        tree.pre_record();
        assert!(!tree.last_inputs.hold_for(&tree), "nothing rolled yet");
        tree.last_inputs.note_rolled();
        tree.pre_record();
        assert!(tree.last_inputs.hold_for(&tree), "the last pass rolled");
        tree.pre_record();
        assert!(
            !tree.last_inputs.hold_for(&tree),
            "the last pass never rolled"
        );
    }
}
