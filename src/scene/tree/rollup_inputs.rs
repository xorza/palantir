//! What [`Tree::compute_rollups`](crate::scene::tree::Tree) read on the
//! pass before, so a pass that records it again keeps the rollups.

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

/// The last pass's recorded columns, swapped out of the tree at
/// `Tree::pre_record` rather than cleared, so the buffers trade places
/// and neither allocates once both are warm.
///
/// Holds every column the rollups read, and compares each the way the
/// rollups read it: the shapes and chrome by the hash they fold in, the
/// rest field by field, at least as strictly as their hashes tell two
/// values apart. Equal inputs give equal rollups, since the rollups are
/// a function of them.
#[derive(Debug, Default)]
pub(crate) struct RollupInputs {
    records: Soa<NodeRecord>,
    bounds_table: Vec<BoundsExtras>,
    panel_table: Vec<PanelExtras>,
    chrome_table: Vec<ChromeRow>,
    shape_hashes: Vec<ContentHash>,
    grid_tracks: Vec<Track>,
    grid_defs: Vec<GridDef>,
    scrollbar_defs: Vec<ResolvedScrollbarsDef>,
    /// Whether the tree's rollups were computed from these columns.
    /// False until a pass computed them.
    rolled: bool,
}

impl RollupInputs {
    /// Take `tree`'s recorded columns as the last pass's, leaving it the
    /// emptied buffers of the pass before. The columns the rollups do
    /// not read are cleared by the caller.
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
    }

    /// Mark the tree's rollups as computed from what it now records,
    /// which the next [`Self::take_from`] makes these columns.
    pub(super) const fn note_rolled(&mut self) {
        self.rolled = true;
    }

    /// Whether `tree` records what the rollups it holds were computed
    /// from.
    pub(super) fn hold_for(&self, tree: &Tree) -> bool {
        if !self.rolled {
            return false;
        }
        let (now, last) = (&tree.records, &self.records);
        // Small columns a frame most often changes first — a paint, a
        // pan, a hover's chrome — so a pass that moved stops before the
        // node columns, which no paint-only change reaches.
        now.len() == last.len()
            && same_bytes(&tree.shapes.hashes, &self.shape_hashes)
            && all_alike(&tree.panel_table, &self.panel_table, |a, b| {
                a == b && a.transform.is_identity() == b.transform.is_identity()
            })
            && all_alike(&tree.chrome_table, &self.chrome_table, |a, b| {
                a.hash == b.hash
            })
            && all_alike(&tree.bounds_table, &self.bounds_table, |a, b| a == b)
            && all_alike(&tree.grid_tracks, &self.grid_tracks, |a, b| a == b)
            && all_alike(&tree.grid_defs, &self.grid_defs, |a, b| {
                a.rows == b.rows && a.cols == b.cols
            })
            && all_alike(&tree.scrollbar_defs, &self.scrollbar_defs, |a, b| {
                a.def == b.def && a.content == b.content
            })
            && same_bytes(now.widget_id(), last.widget_id())
            && same_bytes(now.subtree_end(), last.subtree_end())
            && same_bytes(now.shape_span(), last.shape_span())
            && same_bytes(now.attrs(), last.attrs())
            && same_bytes(now.extras(), last.extras())
            && same_bytes(now.layout(), last.layout())
    }
}

/// Whether two columns of plain words hold the same bytes — one
/// `memcmp`, for the columns whose equality is their bits.
fn same_bytes<T: bytemuck::NoUninit>(now: &[T], last: &[T]) -> bool {
    bytemuck::cast_slice::<T, u8>(now) == bytemuck::cast_slice::<T, u8>(last)
}

/// Whether `alike` holds for every pair of `now` and `last`, folded
/// without a branch per pair so a run of them compares as vectors, and
/// a pass that changed still stops at the first chunk that differs.
fn all_alike<T>(now: &[T], last: &[T], alike: impl Fn(&T, &T) -> bool) -> bool {
    const CHUNK: usize = 64;
    now.len() == last.len()
        && now
            .chunks(CHUNK)
            .zip(last.chunks(CHUNK))
            .all(|(a, b)| a.iter().zip(b).fold(true, |all, (x, y)| all & alike(x, y)))
}

/// Where `Tree::assert_rollups_hold` keeps the rollups a pass kept while
/// it computes them again beside them.
#[cfg(debug_assertions)]
#[derive(Debug, Default)]
pub(crate) struct RollupCheck {
    pub(super) rollups: SubtreeRollups,
    pub(super) container_text: FixedBitSet,
}
