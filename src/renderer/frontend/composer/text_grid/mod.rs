//! Spatial index over text-rect AABBs, used by the composer's
//! paint-order overlap checks. Replaces a flat `Vec<URect>` linear scan
//! that dominated compose time in text-dense UIs.
//!
//! **The shape of the real workload**, from instrumenting the
//! `frame/*_cpu` arms (25–63 M queries each): ~70–80 rects live at query
//! time (peak 207), 24–34% of queries survive the union pre-reject, and
//! a surviving query walks 7–12 tiles to perform ~0.2–1.6 rect tests.
//! `spill` never receives a single entry, and `rects` never approaches
//! [`TILE_INDEX_CAPACITY`] — both are correctness fallbacks that no
//! realistic frame reaches, not tuning knobs.
//!
//! **Why a tiled index and not something simpler**, all measured on
//! `composer/text_grid_realistic` (200 labels, µs per round):
//!
//! - Union pre-reject + linear scan, i.e. what `higher_kind.rs` does:
//!   **56.7 vs 7.0**. The pre-reject alone does not carry it; at ~75
//!   live rects the surviving third of queries scan far too much. Whole
//!   frames move 1.7% (`scrolling_cpu`) to 3.4% (`cached_cpu`).
//! - A conservative coverage bitmap (one bit per cell, no rect lists, no
//!   spill — legal because a false positive only costs a flush) looks
//!   like a rout at 64-px cells, **4.2 vs 7.0**, until you count
//!   flushes: **35 per compose against 11**, i.e. triple the draw calls.
//!   Shrink cells until behaviour matches (16 px, 11 flushes) and it is
//!   **10% slower than the grid** on `cached_cpu`. Exactness is the
//!   feature being paid for here.
//! - Sorted sweep, interval tree, BVH: all need an O(n log n) build
//!   every frame for n ≈ 200, against a ~7 µs total budget.

#[cfg(feature = "bench")]
pub(crate) mod bench;

use crate::primitives::urect::URect;
use glam::UVec2;

/// Physical-pixel size of one tile in [`TextRectGrid`]. Each text rect
/// is registered into every tile it overlaps; each overlap query walks
/// the tiles a quad covers and intersects against per-tile rect lists.
///
/// 64 px is a measured optimum, not a guess — `composer/text_grid_realistic`
/// sweeps it (µs per round, 1920×1080):
///
/// | labels | 32 px | 64 px | 128 px | 256 px |
/// | ------ | ----- | ----- | ------ | ------ |
/// | 64     | 2.53  | 2.20  | 2.23   | 5.16   |
/// | 200    | 8.15  | 7.02  | 7.35   | 36.1   |
/// | 600    | 27.96 | 51.60 | 211.4  | 379.3  |
///
/// Bigger tiles do not degrade gracefully: past ~200 labels occupancy
/// crosses [`TILE_CAP`] and the overflow lands in [`TextRectGrid::spill`],
/// which every later query scans linearly — that is the 128/256 px
/// blow-up, not cache behaviour. Smaller tiles push that cliff out at a
/// steady ~15% cost in the common case.
pub(crate) const TILE_SIZE: u32 = 64;
const TILE_INDEX_CAPACITY: usize = u16::MAX as usize + 1;

/// Per-tile inline capacity. Sized empirically from the
/// `frame/resizing` workload (dense UI at 32× bench scale, viewport
/// 3840×4800 phys px): observed max occupancy was **3**. A tile past
/// capacity diverts to the shared [`TextRectGrid::spill`] list, so
/// pathological text-dense workloads (spreadsheet grids with tiny fonts
/// and no padding) stay functional — they degrade to a linear scan of
/// the spilled indices rather than panicking.
///
/// The headroom over that observed 3 is what stops the degradation from
/// being a cliff, so do not trim it to fit the measurement: dropping to
/// `4` buys 3% at 64 labels and costs **4.2×** at 600
/// (`composer/text_grid_realistic`: 216.9 µs against 51.6 µs), because
/// the overflow starts landing in `spill`.
pub(crate) const TILE_CAP: usize = 8;

/// Spatial index over the open batch's text-rect AABBs. Replaces a
/// flat `Vec<URect>` linear scan that dominated compose time in
/// text-dense UIs. Backed by a row-major grid of tiles
/// ([`TILE_SIZE`] phys px); each rect lives in the tiles it covers,
/// each query walks only the tiles its rect overlaps and may visit a
/// rect twice for rects spanning >1 tile — fine, we early-exit on
/// first hit so duplicate visits cost only constant-factor false
/// positives.
///
/// Tile storage is flat SoA (`lens` + fixed `slots` rows) rather than
/// a `Vec<TinyVec>`: `push`/`any_overlap` are the composer's hottest
/// per-text/per-quad loops, and the inline/heap tag dispatch TinyVec
/// pays on every access profiled at ~2% of the frame on its own.
#[derive(Debug, Default)]
pub(crate) struct TextRectGrid {
    cols: u32,
    rows: u32,
    /// Per-tile occupancy (`0..=TILE_CAP`), row-major
    /// `lens[ty * cols + tx]`. Parallel to `slots`. Reset per batch via
    /// the `touched` walk.
    lens: Vec<u8>,
    /// Per-tile inline rect-index rows; only the first `lens[t]`
    /// entries of `slots[t]` are live.
    slots: Vec<[u16; TILE_CAP]>,
    /// Rect indices whose tile row was full at push time. Checked
    /// linearly (exact rect intersect, no tile pruning) by every query
    /// while non-empty — correct because the tile walk is only an
    /// acceleration structure; empty in any realistic workload.
    pub(crate) spill: Vec<u16>,
    /// Indices (into `lens`/`slots`) that received at least one `push`
    /// this frame — the set we walk on [`Self::clear`] instead of the
    /// full row-major grid. A tile is recorded the first time it
    /// transitions from empty to non-empty within a frame; subsequent
    /// pushes to the same tile skip the record. Capacity is retained
    /// across frames.
    ///
    /// Profiling motivation: `Composer::compose` was spending ~37% of
    /// its self-time clearing all ~4500 tiles every frame (4K viewport
    /// / 64-px tiles), even though only ~100-300 actually held
    /// anything in the bench fixture. Tracking touches drops the
    /// per-frame clear walk to the tiles we genuinely touched.
    touched: Vec<u32>,
    /// All rects inserted into the current batch, in insertion order.
    /// `tiles` indexes the first [`TILE_INDEX_CAPACITY`] rects; any
    /// remaining tail is queried linearly.
    rects: Vec<URect>,
    /// Union AABB of every rect in `rects`. O(1) pre-reject for
    /// [`Self::any_overlap`]: a query outside the union can't hit any
    /// rect, so the tile walk (scattered 32-byte bucket loads from a
    /// grid too big for L1) is skipped entirely. Zero-sized = empty.
    pub(super) union: URect,
}

impl TextRectGrid {
    /// Reshape to cover `viewport` and reset all state. Called once
    /// per frame at compose start. Cheap when the viewport hasn't
    /// changed (no allocation — the outer `Vec` is already sized).
    pub(crate) fn start_frame(&mut self, viewport: UVec2) {
        let cols = viewport.x.div_ceil(TILE_SIZE).max(1);
        let rows = viewport.y.div_ceil(TILE_SIZE).max(1);
        let want = (cols * rows) as usize;
        // Grow-only — never shrink. A smaller-viewport frame reuses
        // the larger backing vectors; tiles beyond the active grid
        // never get touched because `push` clamps indices to
        // `cols - 1` / `rows - 1`. `touched` stores absolute indices
        // into `lens`/`slots`, so `clear` works the same regardless of
        // how `cols × rows` map onto positions inside the vecs.
        //
        // Profiling motivation: the resize-arm bench cycles through
        // 4 different viewports per frame; an unconditional clear +
        // resize sweep over every tile dominated `Composer::compose`
        // (~7% of the bench's CPU cycles in the Vec<TinyVec> era).
        if want > self.lens.len() {
            self.lens.resize(want, 0);
            self.slots.resize(want, [0; TILE_CAP]);
        }
        self.cols = cols;
        self.rows = rows;
        self.clear();
    }

    /// Drop every registered rect. Only walks the tiles that actually
    /// got pushed to this frame (`touched`), not the full row-major
    /// grid — `~100-300` tile clears in the dense-text fixture vs
    /// `~4500` on the full sweep.
    pub(crate) fn clear(&mut self) {
        for &i in &self.touched {
            self.lens[i as usize] = 0;
        }
        self.touched.clear();
        self.spill.clear();
        self.rects.clear();
        self.union = URect::ZERO;
    }

    /// Register `r`. No-op for zero-area input (degenerate text rects
    /// can't intersect anything anyway).
    pub(crate) fn push(&mut self, r: URect) {
        if r.is_paint_empty() {
            return;
        }
        let idx = self.rects.len();
        self.rects.push(r);
        self.union = self.union.union(r);
        // Preserve compact tile buckets for realistic batches; an
        // exceptional tail remains correct without widening every tile.
        if idx >= TILE_INDEX_CAPACITY {
            return;
        }
        let idx = idx as u16;
        let max_x = self.cols - 1;
        let max_y = self.rows - 1;
        let cx0 = (r.min.x / TILE_SIZE).min(max_x);
        let cy0 = (r.min.y / TILE_SIZE).min(max_y);
        let cx1 = ((r.max().x - 1) / TILE_SIZE).min(max_x);
        let cy1 = ((r.max().y - 1) / TILE_SIZE).min(max_y);
        let mut spilled = false;
        for ty in cy0..=cy1 {
            let row = ty * self.cols;
            for tx in cx0..=cx1 {
                let tile_idx = (row + tx) as usize;
                let len = self.lens[tile_idx] as usize;
                if len < TILE_CAP {
                    self.slots[tile_idx][len] = idx;
                    self.lens[tile_idx] = (len + 1) as u8;
                    // First touch this frame? Track for the next
                    // `clear` so we don't have to walk the whole grid.
                    if len == 0 {
                        self.touched.push(tile_idx as u32);
                    }
                } else {
                    spilled = true;
                }
            }
        }
        // One entry however many of the spanned tiles were full: the
        // spill list is scanned tile-blind with an exact rect intersect,
        // so a second copy can only make every future query re-test the
        // same rect. Appended after the loop rather than per full tile.
        if spilled {
            self.spill.push(idx);
        }
    }

    /// `true` if any registered rect intersects `q`. Returns on first
    /// hit. Walks every tile in `q`'s tile range and checks each
    /// tile's rect list — typical workload visits 1-4 tiles with 1-3
    /// rects each (avg total: ~4-8 intersect tests vs ~120 for the
    /// old flat scan).
    pub(crate) fn any_overlap(&self, q: URect) -> bool {
        // The union check subsumes the empty-grid case (an empty grid's
        // zero-sized union intersects nothing).
        if q.is_paint_empty() || !self.union.intersects(q) {
            return false;
        }
        let max_x = self.cols - 1;
        let max_y = self.rows - 1;
        let cx0 = (q.min.x / TILE_SIZE).min(max_x);
        let cy0 = (q.min.y / TILE_SIZE).min(max_y);
        let cx1 = ((q.max().x - 1) / TILE_SIZE).min(max_x);
        let cy1 = ((q.max().y - 1) / TILE_SIZE).min(max_y);
        for ty in cy0..=cy1 {
            let row = ty * self.cols;
            for tx in cx0..=cx1 {
                let t = (row + tx) as usize;
                let n = self.lens[t] as usize;
                for &i in &self.slots[t][..n] {
                    if self.rects[i as usize].intersects(q) {
                        return true;
                    }
                }
            }
        }
        // Overflow paths (both empty in realistic workloads): rects
        // whose tile row was full at push time, then rects past the
        // u16 index space entirely.
        if self
            .spill
            .iter()
            .any(|&i| self.rects[i as usize].intersects(q))
        {
            return true;
        }
        self.rects[TILE_INDEX_CAPACITY.min(self.rects.len())..]
            .iter()
            .any(|r| r.intersects(q))
    }
}

#[cfg(test)]
mod tests;
