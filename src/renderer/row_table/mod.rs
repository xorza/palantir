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

use crate::common::eviction_clock::{
    AGE_EVICTIONS_MAX, AGE_SCAN_MAX, ClockSweep, GRACE_FRAMES, Verdict,
};
use crate::common::expiry_window::EXPIRY_FRAMES;
use crate::common::hash::Hasher;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::row_table::bake::LutRowTexels;
use crate::renderer::row_table::counters::RowTableCounters;
use crate::renderer::row_table::dirty_rows::DirtyRows;
use hashbrown::HashTable;
use std::hash::{Hash as _, Hasher as _};
use std::ops::Range;

pub(crate) mod bake;
#[cfg(feature = "internals")]
pub(crate) mod bench;
mod counters;
mod dirty_rows;

/// Rows a device's table starts with, and the smallest rung a shrink lands on: 32 KiB, room for
/// the fallback and 15 ramps, where an app with one gradient held 512 KiB at 256.
pub(crate) const FIRST_ROWS: u32 = 16;

/// Rows a device's table grows to before it evicts, and under which it never ages: a frame's
/// ramps past them take the rows of ramps no frame drew in the grace, as a page list evicts past
/// its soft budget and ages nothing under it.
pub(crate) const SOFT_ROWS: u32 = 256;

/// Growth ceiling of a table no device holds, a host's that paints nothing: wgpu's downlevel
/// `max_texture_dimension_2d` floor.
pub(crate) const DEFAULT_MAX_ROWS: u32 = 2048;

/// Policy ceiling on rows, below the device's `max_texture_dimension_2d`: one pathological frame
/// takes the texture to 8 MiB (4096 rows × 2 KiB) until the decay brings it back. Past it
/// [`LutRow::FALLBACK`] paints magenta, without repainting what other draws captured.
pub(crate) const MAX_ROWS: u32 = 4096;

/// Clean rows a flush folds into the run around them rather than start another copy. A folded row
/// is baked again from its ramp and staged, 0.9 to 12.7 µs on the bench hosts, against a copy's
/// 4.6 to 45 µs, so folding pays up to a gap of 0.9 to 7.2 rows by host and interpolation. 3 holds
/// the worst a gap loses on any host lowest, about 20 µs: the board's linear rows split at a gap of
/// 4, the handheld's Oklab rows folded at 3
/// (`.notes/research/2026-11-12-atlas-pool-measurements.md`, "The gradient flush").
pub(crate) const MERGE_GAP_ROWS: u32 = 3;

/// What a table is made with: the rows it starts at and shrinks by, the rows it grows to before it
/// evicts, and the rows it never passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RowTableConfig {
    pub(crate) first_rows: u32,
    pub(crate) soft_rows: u32,
    pub(crate) max_rows: u32,
}

impl RowTableConfig {
    /// A device's whose largest texture edge is `device_edge` (one LUT row per texture row): its
    /// ceiling the edge clamped to the [`MAX_ROWS`] policy ceiling, and raised to the soft rows.
    pub(crate) fn of_device(device_edge: u32) -> Self {
        Self {
            first_rows: FIRST_ROWS,
            soft_rows: SOFT_ROWS,
            max_rows: device_edge.clamp(SOFT_ROWS, MAX_ROWS),
        }
    }

    /// The smallest rung `first_rows << k` that holds twice `rows` rows, so a shrink lands at half
    /// full or under, as a `Vec` halves.
    const fn rung(self, rows: u32) -> u32 {
        let mut rung = self.first_rows;
        while rung < 2 * rows {
            rung *= 2;
        }
        rung
    }
}

/// A row a ramp holds: the ramp a flush bakes it from, its hash, which the index rehashes through
/// with no second hash of the ramp, and the display frame a build last registered it in.
#[derive(Clone, Copy, Debug)]
struct Row {
    ramp: ColorRamp,
    hash: u64,
    last_use: u64,
}

/// The pool's row table (`src/gpu/AGENTS.md`, Atlases): a ramp to row-id map, whose rows the
/// backend's texture bakes at each flush. Every window's build registers its ramps in the one
/// table the device's pool owns, so the texture and the table are made and lost together. No CPU
/// mirror of the texture: a flush bakes each row of its runs again from its ramp, the clean rows a
/// run folds in among them, which the bake's determinism makes the same bytes.
///
/// Row 0 is the magenta fallback and never evicted; the held rows are `1..=len`, so a free row is
/// the next. A registration takes, in order: its row where the ramp holds one; a free row; under
/// the soft rows, a doubled table's; the row of a ramp no frame drew in the grace, as the page
/// lists' clock finds it; a doubled table's up to the ceiling; else [`LutRow::FALLBACK`]. Above
/// the soft rows, each display frame's [decay](Self::decay) takes rows idle past an expiry window,
/// the last row moving into each place, and drops the capacity once a quarter or less is held.
///
/// ## Why the index is separate from the rows
///
/// An open-addressed table can't survive eviction: a victim is chosen by age, so keys land away
/// from their home slot and the forward-probe invariant dies at the first eviction, making a miss
/// O(capacity). A separate index keeps every operation O(1) and lets [`Self::grow`] leave lookup
/// alone.
#[derive(Debug)]
pub(crate) struct RowTable {
    /// The rows that hold a ramp, found by the ramp, which each [`Row`] holds: a ramp is the exact
    /// bake identity whatever gradient kind paints it, and it is stored once. A pure lookup index,
    /// undisturbed by growth. Refused: a map from ramp to row beside the rows, which held each
    /// resident ramp twice, about 42 B each; a tombstone-free table by hand, which this spot held,
    /// with its bug.
    index: HashTable<u32>,
    /// Row `id`'s at `id - 1`: the held rows, a prefix of the table.
    rows: Vec<Row>,
    /// Rows the table holds, row 0 among them: the height the backend's texture matches.
    capacity: u32,
    config: RowTableConfig,
    /// The display frame the builds register in, which only moves on.
    frame: u64,
    /// Where the clock resumes, over [`Self::rows`].
    hand: u32,
    /// Where the decay's age-out resumes, over [`Self::rows`].
    age_hand: u32,
    /// The display frame a whole turn of the clock found nothing evictable in: within a frame no
    /// row turns evictable, so no registration walks the rows again.
    dry_frame: Option<u64>,
    /// How each registration resolved; zero-sized in a shipping build.
    counters: RowTableCounters,
    /// Rows changed since the last `flush`, uploaded as runs that fold in a gap up to
    /// [`MERGE_GAP_ROWS`]: two bakes far apart upload two rows, not the span between them.
    dirty: DirtyRows,
}

/// What a flush owes the backend's texture: its height, and the dirty rows as sorted runs, one copy
/// each, which it bakes from the table's ramps as it stages them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FlushedRows<'a> {
    runs: &'a [Range<u32>],
    rows: &'a [Row],
    /// The table's row count. The backend makes its texture again where it differs, taking the
    /// rows both hold by a copy on the GPU, as the held rows keep their ids.
    pub(crate) capacity: u32,
}

impl FlushedRows<'_> {
    /// The dirty runs, sorted, each folding in a gap up to [`MERGE_GAP_ROWS`].
    pub(crate) const fn runs(&self) -> &[Range<u32>] {
        self.runs
    }

    /// Bake row `id`, one of [`Self::runs`]', into `out`: row 0 bright magenta (sRGB `#ff00ff`),
    /// any other its ramp. A run's rows are held, as the held rows are a prefix and a run folds in
    /// only the rows between two dirty ones.
    pub(crate) fn bake(&self, id: u32, out: &mut LutRowTexels) {
        if id == 0 {
            // Linear (1, 0, 1, 1); the sRGB framebuffer encodes it to `#ff00ff`.
            out.fill(RgbaF16::new(1.0, 0.0, 1.0, 1.0));
        } else {
            bake::row(&self.rows[row_idx(id)].ramp, out);
        }
    }
}

/// A table no device holds, a host's that paints nothing, capped at [`DEFAULT_MAX_ROWS`].
impl Default for RowTable {
    fn default() -> Self {
        Self::new(RowTableConfig::of_device(DEFAULT_MAX_ROWS))
    }
}

impl RowTable {
    /// An empty table `config` sets, at display frame 0.
    ///
    /// # Panics
    ///
    /// If `config` starts below two rows, row 0 and one a ramp can hold, or its rows do not rise
    /// from the first through the soft to the ceiling.
    pub(crate) fn new(config: RowTableConfig) -> Self {
        assert!(config.first_rows >= 2, "a table holds a row past row 0");
        assert!(config.first_rows <= config.soft_rows);
        assert!(config.soft_rows <= config.max_rows);
        let mut table = Self {
            index: HashTable::new(),
            rows: Vec::new(),
            capacity: 0,
            config,
            frame: 0,
            hand: 0,
            age_hand: 0,
            dry_frame: None,
            counters: RowTableCounters::default(),
            dirty: DirtyRows::default(),
        };
        table.resize(config.first_rows);
        // Other rows start zero on the GPU, so uploading only row 0 is exact.
        table.dirty.mark(0);
        table
    }

    /// Rows the table holds, the height the backend's LUT texture must match.
    pub(crate) const fn capacity(&self) -> u32 {
        self.capacity
    }

    /// The display frame the builds register in.
    pub(crate) const fn frame(&self) -> u64 {
        self.frame
    }

    /// Register from display frame `frame` on, or stay where a build moved the table ahead: a
    /// build takes the frame its submit will belong to, and one whose submit was dropped leaves
    /// the table a frame ahead of the next window's, which only holds its rows a frame longer.
    pub(crate) fn advance_to(&mut self, frame: u64) {
        self.frame = self.frame.max(frame);
    }

    /// The row for `ramp` in this display frame, which the next flush bakes where it is new;
    /// gradients with the same ramp share a row. A row in `1..capacity`, or [`LutRow::FALLBACK`]
    /// where every row is drawn within the grace and the table is at its ceiling.
    pub(crate) fn register(&mut self, ramp: &ColorRamp) -> LutRow {
        self.counters.registrations.bump();
        let hash = ramp_hash(ramp);
        let rows = &self.rows;
        if let Some(&id) = self.index.find(hash, |&id| rows[row_idx(id)].ramp == *ramp) {
            self.counters.hits.bump();
            self.rows[row_idx(id)].last_use = self.frame;
            return LutRow(id);
        }
        let row = Row {
            ramp: *ramp,
            hash,
            last_use: self.frame,
        };
        loop {
            if self.held() + 1 < self.capacity {
                self.rows.push(row);
                return self.claim(self.held());
            }
            if self.capacity < self.config.soft_rows && self.grow(self.config.soft_rows) {
                continue;
            }
            if let Some(id) = self.victim() {
                let evicted = std::mem::replace(&mut self.rows[row_idx(id)], row);
                self.index
                    .find_entry(evicted.hash, |&held| held == id)
                    .expect("an evicted row is indexed")
                    .remove();
                self.counters.evictions.bump();
                return self.claim(id);
            }
            if self.grow(self.config.max_rows) {
                continue;
            }
            self.counters.fallbacks.bump();
            return LutRow::FALLBACK;
        }
    }

    /// The rows that hold a ramp.
    fn held(&self) -> u32 {
        u32::try_from(self.rows.len()).expect("a table's rows are counted in u32")
    }

    /// Index row `id`, which now holds its ramp, and mark it for the next flush.
    fn claim(&mut self, id: u32) -> LutRow {
        debug_assert_ne!(id, 0, "row 0 is the permanent magenta fallback");
        self.counters.bakes.bump();
        let hash = self.rows[row_idx(id)].hash;
        self.index.insert_unique(hash, id, row_hash(&self.rows));
        self.dirty.mark(id);
        LutRow(id)
    }

    /// The row of a ramp no frame drew in the grace, by the clock from where it last stopped;
    /// `None`, latched for the frame, where every row was drawn in it.
    fn victim(&mut self) -> Option<u32> {
        if self.dry_frame == Some(self.frame) {
            return None;
        }
        let (rows, frame) = (&self.rows, self.frame);
        let sweep = ClockSweep::over(rows.len(), self.hand, |idx| {
            if rows[idx].last_use + GRACE_FRAMES < frame {
                Verdict::Take
            } else {
                Verdict::Skip
            }
        });
        self.hand = sweep.hand;
        if sweep.victim.is_none() {
            self.dry_frame = Some(frame);
        }
        sweep.victim.map(|idx| idx + 1)
    }

    /// Double the rows, up to `limit`, reporting whether the table grew. Resident rows keep their
    /// ids (draw payloads hold them), and the backend copies them into its taller texture, so
    /// growth uploads nothing.
    fn grow(&mut self, limit: u32) -> bool {
        let grown = self.capacity.saturating_mul(2).min(limit);
        if grown <= self.capacity {
            return false;
        }
        self.resize(grown);
        self.counters.growths.bump();
        true
    }

    /// Hold `to` rows, every held row among them: the index's room and the dirty rows follow, as
    /// nothing ties their lengths to the capacity in the types.
    fn resize(&mut self, to: u32) {
        debug_assert!(self.held() < to, "a table holds its rows and row 0");
        self.capacity = to;
        // Room for two entries a row, so the index never grows inside a registration: at half
        // full or under, hashbrown rehashes in place when an eviction's tombstones fill it, with
        // no allocation, where an entry a row resized it once a size.
        let room = 2 * to as usize;
        let hasher = row_hash(&self.rows);
        if room > self.index.capacity() {
            self.index.reserve(room - self.index.len(), hasher);
        } else {
            self.index.shrink_to(room, hasher);
        }
        self.dirty.resize(to);
    }

    /// Return what the table holds past its need, at the end of the display frame's first submit,
    /// as the page lists decay: above the soft rows, the rows no frame drew in an expiry window
    /// leave, at most [`AGE_EVICTIONS_MAX`] moving the last row into a place and
    /// [`AGE_SCAN_MAX`] walked, and with a quarter or less held, the capacity drops to the
    /// smallest rung that holds twice the rows. A row moved is baked again in its new place; the
    /// rows a shrink keeps keep their ids, so the texture copies them. Refused: a halving that
    /// renumbered every live row and baked them all in one frame, up to 1023 bakes and 2 MiB
    /// staged.
    pub(crate) fn decay(&mut self) {
        if self.capacity <= self.config.soft_rows {
            return;
        }
        let mut moves = 0;
        let mut walked = 0;
        let walk = self.rows.len().min(AGE_SCAN_MAX);
        while walked < walk && moves < AGE_EVICTIONS_MAX && !self.rows.is_empty() {
            walked += 1;
            let idx = self.age_hand as usize % self.rows.len();
            if self.rows[idx].last_use + EXPIRY_FRAMES < self.frame {
                // The hand stays, on the row moved into the place.
                moves += u32::from(self.expire(idx));
            } else {
                self.age_hand = u32::try_from(idx + 1).expect("a table's rows are counted in u32");
            }
        }
        if 4 * (self.held() + 1) <= self.capacity {
            self.shrink();
        }
    }

    /// Give back all the table can at once, as the system runs short of memory: every row this
    /// display frame did not draw leaves, the last rows moving into their places with no bound,
    /// the capacity drops to the smallest rung that holds twice the rows, and the rows' and the
    /// index's memory past their need goes.
    pub(crate) fn purge(&mut self) {
        let mut idx = 0;
        while idx < self.rows.len() {
            if self.rows[idx].last_use < self.frame {
                self.expire(idx);
            } else {
                idx += 1;
            }
        }
        self.shrink();
        self.rows.shrink_to_fit();
        self.dirty.shrink_to_fit();
    }

    /// Take row `idx + 1` out, the last row moving into its place, dirty there, and its old place
    /// clean, as no flush owes a row that holds no ramp. Whether a row moved.
    fn expire(&mut self, idx: usize) -> bool {
        let id = u32::try_from(idx + 1).expect("a table's rows are counted in u32");
        let gone = self.rows.swap_remove(idx);
        self.index
            .find_entry(gone.hash, |&held| held == id)
            .expect("a held row is indexed")
            .remove();
        self.counters.expiries.bump();
        let last = self.held() + 1;
        self.dirty.clear(last);
        let Some(moved) = self.rows.get(idx) else {
            return false;
        };
        *self
            .index
            .find_mut(moved.hash, |&held| held == last)
            .expect("a held row is indexed") = id;
        self.dirty.mark(id);
        self.counters.moves.bump();
        true
    }

    /// Drop the capacity to the smallest rung that holds twice the rows, where that is smaller.
    fn shrink(&mut self) {
        let rung = self.config.rung(self.held() + 1);
        if rung < self.capacity {
            self.resize(rung);
            self.counters.shrinks.bump();
        }
    }

    /// The rows the backend's texture owes, which clears them: its height and the dirty runs.
    pub(crate) fn flush(&mut self) -> FlushedRows<'_> {
        let Self {
            dirty,
            counters,
            rows,
            capacity,
            ..
        } = self;
        let runs = dirty.take_runs(MERGE_GAP_ROWS);
        let uploaded: u32 = runs.iter().map(|run| run.end - run.start).sum();
        counters
            .rows_uploaded
            .edit(|n| *n = n.saturating_add(uploaded));
        FlushedRows {
            runs,
            rows,
            capacity: *capacity,
        }
    }
}

/// Where row `id`'s [`Row`] sits in [`RowTable::rows`].
const fn row_idx(id: u32) -> usize {
    id as usize - 1
}

/// The index's hash of `ramp`, through the crate's one hasher.
fn ramp_hash(ramp: &ColorRamp) -> u64 {
    let mut hasher = Hasher::new();
    ramp.hash(&mut hasher);
    hasher.finish()
}

/// The hash of an indexed row, which `rows` holds for it, as the table rehashes.
fn row_hash(rows: &[Row]) -> impl Fn(&u32) -> u64 {
    |&id| rows[row_idx(id)].hash
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    #[cfg(test)]
    use crate::common::counters::CounterSet;
    #[cfg(test)]
    use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
    #[cfg(test)]
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    #[cfg(test)]
    use crate::primitives::paint::lut_row::LutRow;
    use crate::renderer::row_table::RowTable;
    #[cfg(test)]
    use crate::renderer::row_table::bake::{LUT_ROW_TEXELS, LutRowTexels};
    #[cfg(test)]
    use crate::renderer::row_table::{FlushedRows, ramp_hash, row_idx};

    impl RowTable {
        /// The rows that hold a ramp, row 0's fallback among them.
        pub(crate) fn live_rows(&self) -> u32 {
            self.held() + 1
        }

        /// Row `row`'s texels as a flush bakes them.
        #[cfg(test)]
        pub(crate) fn row_texels(&mut self, row: u32) -> LutRowTexels {
            let mut out = [RgbaF16::TRANSPARENT; LUT_ROW_TEXELS];
            let Self { rows, capacity, .. } = self;
            FlushedRows {
                runs: &[],
                rows,
                capacity: *capacity,
            }
            .bake(row, &mut out);
            out
        }

        /// The row `ramp` holds, if one does, with no registration.
        #[cfg(test)]
        pub(crate) fn resident_row(&self, ramp: &ColorRamp) -> Option<LutRow> {
            self.index
                .find(ramp_hash(ramp), |&id| self.rows[row_idx(id)].ramp == *ramp)
                .map(|&id| LutRow(id))
        }

        /// The display frame row `row` was last registered in.
        #[cfg(test)]
        pub(crate) fn last_use(&self, row: LutRow) -> u64 {
            self.rows[row_idx(row.0)].last_use
        }

        /// Live index entries, one per held row; a mismatch shows a duplicate bake or a leaked
        /// eviction entry.
        #[cfg(test)]
        pub(crate) fn index_len(&self) -> usize {
            self.index.len()
        }

        #[cfg(test)]
        pub(crate) const fn max_rows(&self) -> u32 {
            self.config.max_rows
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
