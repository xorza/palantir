//! CPU side of the gradient LUT atlas. Bakes stop sequences into LUT rows shared
//! across linear / radial / conic gradients; the shader derives `t` per fragment.
//! See [`bake::row`] and [`CpuGradientAtlas::register`].
//!
//! Each baked row is 256 [`RgbaF16`] texels (2048 bytes), **premultiplied
//! linear-RGB**, uploaded to an `Rgba16Float` texture with no auto-decode.
//! Premultiplied so the sampler blends the way the stops do (see
//! [`crate::primitives::paint::brush`]); the shader makes the sample straight
//! before multiplying by the fill colour.
//!
//! f16, not u8: a dark stop linearises tiny (`#1a1a2e`'s red ≈ 3/255), so an 8-bit
//! linear row would band `#1a1a2e → #4c5cdb` into ~16 steps over 256 texels. Only
//! the 8-bit sRGB framebuffer quantises. See `dark_gradient_row_has_no_banding`.
//!
//! Stops are sRGB bytes; `bake::row` decodes each to linear once per row, outside
//! the texel loop.
//!
//! - [`Interpolation::Linear`](crate::primitives::paint::brush::gradient::Interpolation::Linear):
//!   physically linear blend; dips at the midpoint of saturated complementary pairs.
//! - [`Interpolation::Oklab`](crate::primitives::paint::brush::gradient::Interpolation::Oklab):
//!   stops converted to Oklab once; the texel loop lerps and runs only
//!   `oklab::to_linear`. Perceptually uniform; the CSS Color 4 default.

use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::gradient_atlas::bake::{LUT_ROW_TEXELS, LutRowTexels};
use crate::renderer::gradient_atlas::counters::GradientAtlasCounters;
use crate::renderer::gradient_atlas::mru_list::MruList;
use rustc_hash::FxHashMap;

pub(crate) mod bake;
#[cfg(feature = "bench")]
pub(crate) mod bench;
mod counters;
mod mru_list;
pub(crate) mod shared_gradient_atlas;

/// Rows the LUT atlas texture starts with. Row 0 is a debug-magenta fallback, so a
/// `fill_lut_row = 0` from a bug paints obviously wrong. The atlas doubles
/// ([`CpuGradientAtlas::grow`]) when a frame registers more gradients than fit.
pub(crate) const INITIAL_ATLAS_ROWS: u32 = 256;

/// Growth ceiling when no device limit is known: wgpu's downlevel
/// `max_texture_dimension_2d` floor.
pub(crate) const DEFAULT_MAX_ATLAS_ROWS: u32 = 2048;

/// Policy ceiling on rows, below the device's `max_texture_dimension_2d`. Growth
/// is a one-way ratchet, so a 16384-row bound would let one pathological frame
/// pin 32 MB of rows plus a 32 MB texture for the process. Past it
/// [`LutRow::FALLBACK`] paints magenta, without repainting what other draws captured.
pub(crate) const MAX_ATLAS_ROWS: u32 = 4096;

/// One row's bookkeeping. `baked` stays a separate `Vec<LutRowTexels>` so the
/// upload is a single reinterpret to `&[u8]`.
#[derive(Clone, Debug, Default)]
struct RowSlot {
    /// The key this row holds, so eviction can drop its index entry. `None` for a
    /// never-claimed row and for row 0 (not in the MRU list).
    key: Option<ColorRamp>,
    /// The [`CpuGradientAtlas::epoch`] the row was last registered in. A current-epoch
    /// row cannot be evicted: its `LutRow` is in this frame's draw payloads. `0` means
    /// never claimed, hence the epoch starts at 1.
    epoch: u64,
}

/// CPU side of the gradient LUT atlas: baked row bytes plus a bake-key to row-id
/// map; the backend mirrors it into a wgpu texture via [`Self::flush`].
///
/// Row 0 is the magenta fallback and never evicted. With no free row, the
/// least-recently-used one is evicted and re-baked in place, excluding rows
/// registered since the last flush; if all are exempt the atlas [grows](Self::grow).
///
/// ## Why the index is separate from the rows
///
/// An open-addressed table can't survive eviction: a victim is chosen by recency,
/// so keys land away from their home slot and the forward-probe invariant dies at
/// the first eviction, making a miss O(capacity). A separate index keeps every
/// operation O(1) and lets [`Self::grow`] leave lookup alone.
#[derive(Debug)]
pub(crate) struct CpuGradientAtlas {
    /// Ramp to the row holding it; a ramp is the exact bake identity whatever gradient
    /// kind paints it. A pure lookup index, undisturbed by eviction and growth.
    index: FxHashMap<ColorRamp, u32>,
    slots: Vec<RowSlot>,
    /// Baked LUT row bytes by row id; one contiguous allocation.
    baked: Vec<LutRowTexels>,
    /// Recency order over rows `1..capacity`; see [`mru_list`] for why its tail alone
    /// answers "what may be evicted".
    mru: MruList,
    /// Hard row ceiling: `min(device max_texture_dimension_2d, MAX_ATLAS_ROWS)`.
    max_rows: u32,
    /// Registration epoch, bumped per [`Self::flush`] (the per-submit boundary). The
    /// atlas is shared across windows, but each re-registers its gradients before its
    /// own flush, so exempting rows registered since the last flush is safe.
    epoch: u64,
    /// How each registration resolved; zero-sized in a shipping build.
    counters: GradientAtlasCounters,
    /// Contiguous row range changed since the last `flush`; uploaded in one
    /// `write_texture` since per-call cost dominates. Scattered rows upload the
    /// min..=max span.
    dirty: Option<DirtyRows>,
}

#[derive(Clone, Copy, Debug)]
struct DirtyRows {
    first: u32,
    last: u32,
}

/// A contiguous span of freshly baked rows for GPU upload; `bytes` starts at row
/// `first_row` and covers whole rows.
#[derive(Debug)]
pub(crate) struct FlushedRows<'a> {
    pub(crate) first_row: u32,
    pub(crate) bytes: &'a [u8],
    /// The atlas's current row count. The backend recreates its texture when it
    /// differs; [`CpuGradientAtlas::grow`] dirties every row, so the same upload
    /// refills it.
    pub(crate) total_rows: u32,
}

impl Default for CpuGradientAtlas {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ATLAS_ROWS)
    }
}

impl CpuGradientAtlas {
    /// Atlas capped at `max_rows`, raised to [`INITIAL_ATLAS_ROWS`] if lower.
    pub(crate) fn new(max_rows: u32) -> Self {
        // Row 0 alone: the permanent fallback, not an MRU member, so `resize_rows` can
        // reach `INITIAL_ATLAS_ROWS` from row 1 as every growth does.
        let mut atlas = Self {
            index: FxHashMap::default(),
            slots: vec![RowSlot::default()],
            baked: vec![[RgbaF16::TRANSPARENT; LUT_ROW_TEXELS]],
            mru: MruList::seeded(1),
            max_rows: max_rows.max(INITIAL_ATLAS_ROWS),
            epoch: 1,
            dirty: None,
            counters: GradientAtlasCounters::default(),
        };
        atlas.resize_rows(INITIAL_ATLAS_ROWS);
        atlas.init_row_zero_magenta();
        atlas
    }

    pub(crate) const fn capacity(&self) -> u32 {
        self.baked.len() as u32
    }

    /// Fill row 0 with bright magenta (sRGB `#ff00ff`).
    fn init_row_zero_magenta(&mut self) {
        // Linear (1, 0, 1, 1); the sRGB framebuffer encodes it to `#ff00ff`.
        let magenta = RgbaF16::new(1.0, 0.0, 1.0, 1.0);
        self.baked[0].fill(magenta);
        // Other rows start zero on the GPU, so uploading only row 0 is exact.
        self.mark_row_dirty(0);
    }

    /// Find-or-bake the row for `ramp`; gradients with the same ramp share a row.
    /// Returns a row id in `1..capacity`, moved to the MRU head on every call.
    ///
    /// Once every row is occupied: claim the tail if not referenced this epoch, else
    /// [grow](Self::grow), else return [`LutRow::FALLBACK`].
    pub(crate) fn register(&mut self, ramp: &ColorRamp) -> LutRow {
        self.counters.registrations.bump();
        let key = *ramp;
        // Hit: mark the row referenced this epoch; it must not be evicted before upload.
        if let Some(&row) = self.index.get(&key) {
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
    /// Resident rows keep their ids (draw payloads hold them); new rows join the MRU
    /// list at the eviction end.
    fn grow(&mut self) -> bool {
        let capacity = self.capacity();
        let grown = capacity.saturating_mul(2).min(self.max_rows);
        if grown <= capacity {
            return false;
        }
        self.resize_rows(grown);
        self.counters.growths.bump();
        // The backend replaces its texture at the new height and wgpu zero-initializes
        // it, so every row must re-upload.
        self.dirty = Some(DirtyRows {
            first: 0,
            last: grown - 1,
        });
        debug_assert!(self.mru.is_well_formed(), "growth corrupted the MRU list");
        true
    }

    /// Move every per-row column to `to` rows, together: the one resize site, since
    /// `slots`, `baked` and the MRU list can't share a length invariant in the types.
    fn resize_rows(&mut self, to: u32) {
        // Read before anything moves: the MRU list extends from the old row count.
        let from = self.capacity();
        self.slots.resize(to as usize, RowSlot::default());
        self.baked
            .resize(to as usize, [RgbaF16::TRANSPARENT; LUT_ROW_TEXELS]);
        self.mru.extend_to(from, to);
        debug_assert_eq!(
            self.slots.len(),
            self.baked.len(),
            "per-row columns must resize together",
        );
    }

    /// Bake `key` into `row` and take over the slot: index entry, recency, epoch,
    /// dirty range. Shared by the free-row and evict arms.
    fn claim_row(&mut self, row: u32, key: ColorRamp) -> LutRow {
        debug_assert_ne!(row, 0, "row 0 is the permanent magenta fallback");
        // Evicting: drop the outgoing index entry, or a lookup would resolve to another bake.
        let displaced = self.slots[row as usize].key.replace(key);
        if let Some(evicted) = displaced {
            self.index.remove(&evicted);
        }
        self.counters.bake(displaced.is_some());
        bake::row(&key, &mut self.baked[row as usize]);
        self.index.insert(key, row);
        self.touch(row);
        self.mark_row_dirty(row);
        LutRow(row)
    }

    fn mark_row_dirty(&mut self, row: u32) {
        self.dirty = Some(match self.dirty {
            None => DirtyRows {
                first: row,
                last: row,
            },
            Some(d) => DirtyRows {
                first: d.first.min(row),
                last: d.last.max(row),
            },
        });
    }

    /// Return the contiguous dirty row span for one-shot upload and clear it. Also
    /// bumps the registration epoch: rows registered since the last flush are
    /// eviction-exempt until after this one.
    pub(crate) fn flush(&mut self) -> Option<FlushedRows<'_>> {
        self.epoch = self.epoch.wrapping_add(1);
        let dirty = self.dirty.take()?;
        let total_rows = self.capacity();
        let rows = &self.baked[dirty.first as usize..=dirty.last as usize];
        let uploaded = rows.len() as u32;
        self.counters
            .rows_uploaded
            .edit(|n| *n = n.saturating_add(uploaded));
        Some(FlushedRows {
            first_row: dirty.first,
            bytes: bytemuck::cast_slice(rows),
            total_rows,
        })
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use super::*;

    impl CpuGradientAtlas {
        /// Whether this epoch's registered rows form a head prefix of the MRU list, which
        /// [`Self::register`] relies on to decide eviction from the tail.
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
        pub(crate) fn index_len(&self) -> usize {
            self.index.len()
        }

        pub(crate) fn max_rows(&self) -> u32 {
            self.max_rows
        }

        /// The row `key`'s gradient occupies, straight from the index, so a test can tell
        /// "same row" from "re-baked" without registering.
        pub(crate) fn resident_row(&self, ramp: &ColorRamp) -> Option<u32> {
            self.index.get(ramp).copied()
        }
    }
}

#[cfg(test)]
mod tests;
