//! The gradient row table, its CPU half. Bakes stop sequences into LUT rows shared
//! across linear / radial / conic gradients; the shader derives `t` per fragment.
//! See [`bake::row`] and [`RowTable::register`].
//!
//! Each baked row is 256 [`RgbaF16`] texels (2048 bytes), **premultiplied
//! linear-RGB**, uploaded to an `Rgba16Float` texture with no auto-decode.
//! Premultiplied so the sampler blends the way the stops do (see
//! [`crate::primitives::paint::brush`]); the shader makes the sample straight
//! before multiplying by the fill color.
//!
//! f16, not u8: a dark stop linearizes tiny (`#1a1a2e`'s red ≈ 3/255), so an 8-bit
//! linear row would band `#1a1a2e → #4c5cdb` into ~16 steps over 256 texels. Only
//! the 8-bit sRGB framebuffer quantizes. See `dark_gradient_row_has_no_banding`.
//!
//! Stops are sRGB bytes; `bake::row` decodes each to linear once per row, outside
//! the texel loop.
//!
//! - [`Interpolation::Linear`](crate::primitives::paint::brush::gradient::Interpolation::Linear):
//!   physically linear blend; dips at the midpoint of saturated complementary pairs.
//! - [`Interpolation::Oklab`](crate::primitives::paint::brush::gradient::Interpolation::Oklab):
//!   stops converted to Oklab once; the texel loop lerps and runs only
//!   `Oklab::to_color`. Perceptually uniform; the CSS Color 4 default.

use crate::common::expiry_window::{EXPIRY_FRAMES, ExpiryWindow};
use crate::common::hash::Hasher;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::row_table::bake::{LUT_ROW_TEXELS, LutRowTexels};
use crate::renderer::row_table::counters::RowTableCounters;
use crate::renderer::row_table::dirty_rows::DirtyRows;
use crate::renderer::row_table::mru_list::MruList;
use hashbrown::HashTable;
use std::hash::{Hash as _, Hasher as _};
use std::ops::Range;

pub(crate) mod bake;
#[cfg(feature = "internals")]
pub(crate) mod bench;
mod counters;
mod dirty_rows;
mod mru_list;

/// Rows the table starts with, and the fewest a halving leaves. Row 0 is a
/// debug-magenta fallback, so a `fill_lut_row = 0` from a bug paints obviously wrong. The table
/// doubles ([`RowTable::grow`]) when a frame registers more gradients than fit.
pub(crate) const INITIAL_ROWS: u32 = 256;

/// Growth ceiling of a table no device holds, a host's that paints nothing: wgpu's downlevel
/// `max_texture_dimension_2d` floor.
pub(crate) const DEFAULT_MAX_ROWS: u32 = 2048;

/// Policy ceiling on rows, below the device's `max_texture_dimension_2d`: one pathological frame
/// takes the texture to 8 MiB (4096 rows × 2 KiB) until the halvings bring it back. Past it
/// [`LutRow::FALLBACK`] paints magenta, without repainting what other draws captured.
pub(crate) const MAX_ROWS: u32 = 4096;

/// Clean rows a flush folds into the run around them rather than start another copy. A folded row
/// is baked again from its ramp and staged, 0.9 to 12.7 µs on the bench hosts, against a copy's
/// 4.6 to 45 µs, so folding pays up to a gap of 0.9 to 7.2 rows by host and interpolation. 3 holds
/// the worst a gap loses on any host lowest, about 20 µs: the board's linear rows split at a gap of
/// 4, the handheld's Oklab rows folded at 3
/// (`.notes/research/2026-11-12-atlas-pool-measurements.md`, "The gradient flush").
pub(crate) const MERGE_GAP_ROWS: u32 = 3;

/// One row's bookkeeping: the ramp a flush bakes it from, and when a frame last registered it.
#[derive(Clone, Debug, Default)]
struct RowSlot {
    /// The ramp this row holds, which the index hashes and compares through. `None` for a
    /// never-claimed row and for row 0 (not in the MRU list).
    key: Option<ColorRamp>,
    /// The [`RowTable::epoch`] the row was last registered in. A current-epoch
    /// row cannot be evicted: its `LutRow` is in this frame's draw payloads. `0` means
    /// never claimed, hence the epoch starts at 1.
    epoch: u64,
}

/// The pool's row table (`src/gpu/AGENTS.md`, Atlases): a ramp to row-id map, whose rows
/// [`Self::flush`] bakes for the backend's texture. Every window's build registers its ramps in
/// the one table the device's pool owns, so the texture and the table are made and lost together.
/// No CPU mirror of the texture: a flush bakes each row of its runs again from its ramp, the clean
/// rows a run folds in among them, which the bake's determinism makes the same bytes.
///
/// Row 0 is the magenta fallback and never evicted. With no free row, the
/// least-recently-used one is evicted and claimed in place, excluding rows
/// registered since the last flush; if all are exempt the table [grows](Self::grow). At the
/// start of a frame that ends an expiry window with fewer than a quarter of the rows registered
/// in it, the table [halves](Self::halve): those rows renumbered low, the rest dropped.
///
/// ## Why the index is separate from the rows
///
/// An open-addressed table can't survive eviction: a victim is chosen by recency,
/// so keys land away from their home slot and the forward-probe invariant dies at
/// the first eviction, making a miss O(capacity). A separate index keeps every
/// operation O(1) and lets [`Self::grow`] leave lookup alone.
#[derive(Debug)]
pub(crate) struct RowTable {
    /// The rows that hold a ramp, found by the ramp, which each row's [`RowSlot::key`] holds: a
    /// ramp is the exact bake identity whatever gradient kind paints it, and it is stored once.
    /// A pure lookup index, undisturbed by eviction and growth. Refused: a map from ramp to row
    /// beside the slots, which held each resident ramp twice, about 42 B each.
    index: HashTable<u32>,
    slots: Vec<RowSlot>,
    /// Recency order over rows `1..capacity`; see [`mru_list`] for why its tail alone
    /// answers "what may be evicted".
    mru: MruList,
    /// Hard row ceiling: `min(device max_texture_dimension_2d, MAX_ROWS)`.
    max_rows: u32,
    /// Registration epoch, bumped per [`Self::flush`] (the per-submit boundary). The
    /// table is shared across windows, but each re-registers its gradients before its
    /// own flush, so exempting rows registered since the last flush is safe.
    epoch: u64,
    /// How each registration resolved; zero-sized in a shipping build.
    counters: RowTableCounters,
    /// Rows changed since the last `flush`, uploaded as runs that fold in a gap up to
    /// [`MERGE_GAP_ROWS`]: two bakes far apart upload two rows, not the span between them.
    dirty: DirtyRows,
    /// The rows a flush bakes, its runs one after another; retained, so a flush allocates
    /// nothing past the largest it has staged.
    staging: Vec<LutRowTexels>,
    /// The expiry window over [`Self::epoch`] whose end may halve the table.
    window: ExpiryWindow,
    /// The rows a halving keeps, retained.
    kept: Vec<RowSlot>,
}

/// Freshly baked rows for GPU upload, as sorted runs: one copy each.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FlushedRows<'a> {
    runs: &'a [Range<u32>],
    /// Each run's rows, baked, the runs one after another.
    rows: &'a [LutRowTexels],
    /// The table's current row count. The backend makes its texture again when it differs: a
    /// grown table's takes the old texture's rows by a copy on the GPU, as resident rows keep
    /// their ids; a halved table's takes none, as its flush holds every row it kept.
    pub(crate) total_rows: u32,
}

impl<'a> FlushedRows<'a> {
    /// Each run and its rows' texels as bytes, in row order.
    pub(crate) fn copies(&self) -> impl Iterator<Item = (Range<u32>, &'a [u8])> + Clone + 'a {
        let rows = self.rows;
        self.runs.iter().scan(0, move |at: &mut usize, run| {
            let len = (run.end - run.start) as usize;
            let bytes = bytemuck::cast_slice(&rows[*at..*at + len]);
            *at += len;
            Some((run.clone(), bytes))
        })
    }
}

/// A table no device holds, a host's that paints nothing, capped at [`DEFAULT_MAX_ROWS`].
impl Default for RowTable {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ROWS)
    }
}

impl RowTable {
    /// An empty table whose growth ceiling is a device's largest texture edge `device_edge` (one
    /// LUT row per texture row), clamped to the [`MAX_ROWS`] policy ceiling and raised to
    /// [`INITIAL_ROWS`].
    pub(crate) fn new(device_edge: u32) -> Self {
        // Row 0 alone: the permanent fallback, not an MRU member, so `resize_rows` can
        // reach `INITIAL_ROWS` from row 1 as every growth does.
        let mut table = Self {
            index: HashTable::new(),
            slots: vec![RowSlot::default()],
            mru: MruList::seeded(1),
            max_rows: device_edge.clamp(INITIAL_ROWS, MAX_ROWS),
            epoch: 1,
            dirty: DirtyRows::default(),
            counters: RowTableCounters::default(),
            staging: Vec::new(),
            window: ExpiryWindow::NEW,
            kept: Vec::new(),
        };
        table.resize_rows(INITIAL_ROWS);
        // Other rows start zero on the GPU, so uploading only row 0 is exact.
        table.mark_row_dirty(0);
        table
    }

    /// Rows the table holds, the height the backend's LUT texture must match. Starts at
    /// [`INITIAL_ROWS`], doubles and halves.
    pub(crate) const fn capacity(&self) -> u32 {
        self.slots.len() as u32
    }

    /// Find-or-claim the row for `ramp`, which the next flush bakes; gradients with the same ramp
    /// share a row.
    /// Returns a row id in `1..capacity`, moved to the MRU head on every call.
    ///
    /// Once every row is occupied: claim the tail if not referenced this epoch, else
    /// [grow](Self::grow), else return [`LutRow::FALLBACK`].
    pub(crate) fn register(&mut self, ramp: &ColorRamp) -> LutRow {
        self.counters.registrations.bump();
        let key = *ramp;
        // Hit: mark the row referenced this epoch; it must not be evicted before upload.
        if let Some(row) = self.resident_row(&key) {
            self.counters.hits.bump();
            self.touch(row);
            return LutRow(row);
        }
        loop {
            // The MRU tail is never-claimed or least-recently-registered, and current-epoch
            // rows form a head prefix, so this one check decides eviction.
            let victim = self.mru.tail();
            if self.slots[victim as usize].epoch != self.epoch {
                return self.claim_row(victim, key);
            }
            // Every row is in use by this frame: grow and retry; new rows land at the tail
            // unclaimed, so this loops at most twice.
            if !self.grow() {
                self.counters.fallbacks.bump();
                return LutRow::FALLBACK;
            }
        }
    }

    /// Mark `row` most recently used and referenced this epoch. Every registration
    /// path goes through here, keeping current-epoch rows a head prefix of the MRU list.
    /// Pinned by `epoch_current_rows_form_an_mru_prefix`.
    #[inline]
    fn touch(&mut self, row: u32) {
        self.mru.touch(row);
        self.slots[row as usize].epoch = self.epoch;
    }

    /// Double the row count (capped at [`Self::max_rows`]), reporting whether it grew.
    /// Resident rows keep their ids (draw payloads hold them), and the backend copies them into
    /// its taller texture, so growth uploads nothing; new rows join the MRU list at the eviction
    /// end.
    fn grow(&mut self) -> bool {
        let capacity = self.capacity();
        let grown = capacity.saturating_mul(2).min(self.max_rows);
        if grown <= capacity {
            return false;
        }
        self.resize_rows(grown);
        self.counters.growths.bump();
        debug_assert!(self.mru.is_well_formed(), "growth corrupted the MRU list");
        true
    }

    /// Move every per-row column to `to` rows, together: the one resize site, since `slots`, the
    /// index's room, the MRU list and the dirty rows can't share a length invariant in the types.
    fn resize_rows(&mut self, to: u32) {
        // Read before anything moves: the MRU list extends from the old row count.
        let from = self.capacity();
        debug_assert!(
            from <= to,
            "rows resize up; a halving starts over from row 1"
        );
        self.slots.resize(to as usize, RowSlot::default());
        // Room for an entry a row, so a registration never grows the table: growth, a cold
        // frame, is where it grows. An eviction's tombstone can still make an insert rehash the
        // table in place, as in any open-addressed table, hashbrown's included.
        self.index
            .reserve(to as usize - self.index.len(), row_hash(&self.slots));
        self.mru.extend_to(from, to);
        self.dirty.resize(to);
    }

    /// Start a frame: at the end of an expiry window over the flushes, halve the table where fewer
    /// than a quarter of its rows were registered in the window. Before the frame registers
    /// anything, so every row id the frame hands out is the halved table's. The window counts
    /// flushes, each a window's submit, so two windows end it twice as fast, which only shortens
    /// the idle wait.
    pub(crate) fn begin_frame(&mut self) {
        if !self.window.ends(self.epoch) {
            return;
        }
        let capacity = self.capacity();
        let live = self
            .slots
            .iter()
            .filter(|slot| is_live(slot, self.epoch))
            .count();
        if capacity > INITIAL_ROWS && (live as u32) < capacity / 4 {
            self.halve();
        }
    }

    /// Halve the rows, keeping the live ones renumbered from row 1 in their recency order and
    /// dropping the rest; every kept row and row 0 dirty, as the backend's halved texture starts
    /// empty. Renumbers every kept ramp's row, so it runs only before a frame registers.
    fn halve(&mut self) {
        let to = self.capacity() / 2;
        let epoch = self.epoch;
        self.kept.clear();
        self.kept.extend(
            self.slots
                .iter()
                .filter(|slot| is_live(slot, epoch))
                .cloned(),
        );
        debug_assert!(self.kept.len() < to as usize, "a halving keeps what fits");
        // Least recent first, so the most recent lands at the MRU head.
        self.kept.sort_by_key(|slot| slot.epoch);
        self.index.clear();
        self.slots.truncate(1);
        self.mru = MruList::seeded(1);
        self.dirty = DirtyRows::default();
        self.resize_rows(to);
        self.mark_row_dirty(0);
        for (row, slot) in (1..).zip(self.kept.drain(..)) {
            let key = slot.key.as_ref().expect("a kept row holds a ramp");
            self.index
                .insert_unique(ramp_hash(key), row, row_hash(&self.slots));
            self.slots[row as usize] = slot;
            self.mru.touch(row);
            self.dirty.mark(row);
        }
        self.counters.halvings.bump();
        debug_assert!(self.mru.is_well_formed(), "halving corrupted the MRU list");
    }

    /// Take over `row` for `key`: index entry, recency, epoch, dirty range, so the next flush
    /// bakes it. Shared by the free-row and evict arms.
    fn claim_row(&mut self, row: u32, key: ColorRamp) -> LutRow {
        debug_assert_ne!(row, 0, "row 0 is the permanent magenta fallback");
        // Evicting: drop the outgoing index entry, or a lookup would resolve to another bake.
        if let Some(evicted) = self.slots[row as usize].key.replace(key) {
            self.index
                .find_entry(ramp_hash(&evicted), |&held| held == row)
                .expect("an evicted row is indexed")
                .remove();
            self.counters.evictions.bump();
        }
        self.counters.bakes.bump();
        self.index
            .insert_unique(ramp_hash(&key), row, row_hash(&self.slots));
        self.touch(row);
        self.mark_row_dirty(row);
        LutRow(row)
    }

    /// The row that holds `ramp`, if one does.
    fn resident_row(&self, ramp: &ColorRamp) -> Option<u32> {
        self.index
            .find(ramp_hash(ramp), |&row| {
                self.slots[row as usize].key.as_ref() == Some(ramp)
            })
            .copied()
    }

    fn mark_row_dirty(&mut self, row: u32) {
        self.dirty.mark(row);
    }

    /// Bake the dirty rows as runs for upload and clear them. Also bumps the registration
    /// epoch: rows registered since the last flush are eviction-exempt until after this one.
    pub(crate) fn flush(&mut self) -> Option<FlushedRows<'_>> {
        self.epoch += 1;
        let total_rows = self.capacity();
        let Self {
            dirty,
            staging,
            counters,
            ..
        } = self;
        let runs = dirty.take_runs(MERGE_GAP_ROWS);
        if runs.is_empty() {
            return None;
        }
        let uploaded: u32 = runs.iter().map(|run| run.end - run.start).sum();
        counters
            .rows_uploaded
            .edit(|n| *n = n.saturating_add(uploaded));
        staging.clear();
        staging.resize(uploaded as usize, [RgbaF16::TRANSPARENT; LUT_ROW_TEXELS]);
        let rows = runs.iter().flat_map(Clone::clone);
        for (row, out) in rows.zip(staging.iter_mut()) {
            bake_row(&self.slots, row, out);
        }
        Some(FlushedRows {
            runs,
            rows: staging,
            total_rows,
        })
    }
}

/// Whether `slot` holds a ramp a frame registered in the expiry window ending at `epoch`.
const fn is_live(slot: &RowSlot, epoch: u64) -> bool {
    slot.key.is_some() && slot.epoch + EXPIRY_FRAMES > epoch
}

/// Bake row `row` of `slots` into `out`: row 0 bright magenta (sRGB `#ff00ff`), a row that holds a
/// ramp the ramp, any other zero, as the GPU texture starts.
fn bake_row(slots: &[RowSlot], row: u32, out: &mut LutRowTexels) {
    match &slots[row as usize].key {
        // Linear (1, 0, 1, 1); the sRGB framebuffer encodes it to `#ff00ff`.
        _ if row == 0 => out.fill(RgbaF16::new(1.0, 0.0, 1.0, 1.0)),
        Some(ramp) => bake::row(ramp, out),
        None => out.fill(RgbaF16::TRANSPARENT),
    }
}

/// The index's hash of `ramp`, through the crate's one hasher.
fn ramp_hash(ramp: &ColorRamp) -> u64 {
    let mut hasher = Hasher::new();
    ramp.hash(&mut hasher);
    hasher.finish()
}

/// The hash of an indexed row, through the ramp `slots` holds for it, as the table rehashes.
fn row_hash(slots: &[RowSlot]) -> impl Fn(&u32) -> u64 {
    |&row| {
        ramp_hash(
            slots[row as usize]
                .key
                .as_ref()
                .expect("an indexed row holds a ramp"),
        )
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use super::*;

    #[cfg(test)]
    impl<'a> FlushedRows<'a> {
        /// The texels of `run`, one of the flush's runs, as bytes.
        pub(crate) fn bytes(&self, run: Range<u32>) -> &'a [u8] {
            self.copies()
                .find(|(copied, _)| *copied == run)
                .expect("a run of the flush")
                .1
        }
    }
    #[cfg(test)]
    use crate::common::counters::CounterSet;

    impl RowTable {
        /// The rows that hold a ramp, row 0's fallback among them.
        pub(crate) fn live_rows(&self) -> u32 {
            u32::try_from(self.index.len()).expect("the rows are counted in u32") + 1
        }

        /// Row `row`'s texels as a flush bakes them.
        #[cfg(test)]
        pub(crate) fn row_texels(&self, row: u32) -> LutRowTexels {
            let mut out = [RgbaF16::TRANSPARENT; LUT_ROW_TEXELS];
            bake_row(&self.slots, row, &mut out);
            out
        }

        /// The bytes of the rows a flush stages, as large as the largest flush.
        pub(crate) const fn staging_bytes(&self) -> u64 {
            (self.staging.capacity() * size_of::<LutRowTexels>()) as u64
        }

        /// Whether this epoch's registered rows form a head prefix of the MRU list, which
        /// [`Self::register`] relies on to decide eviction from the tail.
        #[cfg(test)]
        pub(crate) fn epoch_prefix_holds(&self) -> bool {
            let mut seen_stale = false;
            for row in self.mru.to_vec() {
                match self.slots[row as usize].epoch == self.epoch {
                    true if seen_stale => return false,
                    true => {}
                    false => seen_stale = true,
                }
            }
            true
        }

        /// Live index entries, one per occupied row; a mismatch shows a duplicate bake or
        /// leaked eviction entry.
        #[cfg(test)]
        pub(crate) fn index_len(&self) -> usize {
            self.index.len()
        }

        #[cfg(test)]
        pub(crate) const fn max_rows(&self) -> u32 {
            self.max_rows
        }

        /// `register` calls so far, letting resolver tests prove their per-pass memo suppresses
        /// repeats. Accumulates for the table's life; read a delta.
        #[cfg(test)]
        pub(crate) fn registrations(&self) -> u32 {
            self.counters.counts().registrations
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
