//! CPU side of the gradient LUT atlas. Bakes stop sequences into LUT
//! rows shared across linear / radial / conic gradient variants; the
//! shader does the per-fragment `t` derivation. See [`bake_stops`] and
//! [`CpuGradientAtlas::register_stops`].
//!
//! ## Bake output convention
//!
//! Each baked row is 256 [`ColorF16`] texels = 2048 bytes, **straight
//! (non-premultiplied) linear-RGB** f16. The backend uploads these into
//! an `Rgba16Float` texture (no auto-decode); the shader samples and
//! gets the stored linear value directly as f16-decoded floats.
//! Premultiply happens in the shader on the sampled value — same
//! convention as the rest of the pipeline (see "Colour pipeline" in
//! `AGENTS.md`).
//!
//! f16, not u8: a dark stop linearises to a tiny value (`#1a1a2e`'s red
//! is linear ≈ 0.010 ≈ 3/255), so an 8-bit *linear* row crushes the
//! dark half of a dark→bright gradient onto a handful of integer
//! levels — `#1a1a2e → #4c5cdb` spans red 3..19, ~16 steps over 256
//! texels, i.e. ~16 visible bands. f16 carries ~11 bits of mantissa at
//! that magnitude (ulp ≈ 8e-6), far finer than the per-texel delta, so
//! the row is smooth and only the 8-bit sRGB framebuffer quantises the
//! output. See `dark_gradient_row_has_no_banding`.
//!
//! ## Interpolation spaces
//!
//! Stops live as `ColorU8` (linear u8 storage — the default
//! `From<Color> for ColorU8` is a linear quantize). `bake_stops`
//! decodes each stop to a linear `Color` **once** per row before the
//! 256-texel loop, so the inner loop never re-runs the cubic.
//!
//! - [`Interp::Linear`]: physically correct linear blend. Shows the
//!   classic midpoint dip on saturated complementary pairs (red↔green
//!   muddy brown).
//! - [`Interp::Oklab`]: pre-converts each stop's linear RGB to Oklab
//!   `L/a/b` triplets once at bake time; the texel loop lerps the
//!   triplet and runs only `oklab_to_linear` per texel. Perceptually
//!   uniform; CSS Color 4 default.

use crate::common::hash::Hasher as FxHasher;
use crate::primitives::brush::{GradientStops, Interp};
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::fill_wire::LutRow;
use crate::renderer::gradient_atlas::bake::{LUT_ROW_TEXELS, LutRowTexels, bake_stops};
use std::hash::{Hash, Hasher};

pub(crate) mod bake;
pub(crate) mod handle;

/// Number of rows in the LUT atlas texture. One row per distinct
/// gradient currently in use. Row 0 is reserved as a debug-magenta
/// fallback (so a `fill_lut_row = 0` from a bug paints obviously
/// wrong); real registrations occupy rows 1..ATLAS_ROWS.
pub(crate) const ATLAS_ROWS: u32 = 256;

/// Exact bake identity shared by every gradient variant.
#[derive(Clone, Debug, PartialEq, Eq)]
struct GradientLutKey {
    stops: GradientStops,
    interp: Interp,
}

impl GradientLutKey {
    fn matches(&self, stops: &GradientStops, interp: Interp) -> bool {
        self.interp == interp && self.stops.eq(stops)
    }
}

#[derive(Debug)]
struct GradientAtlasSlot {
    content_hash: u64,
    key: GradientLutKey,
}

/// CPU side of the gradient LUT atlas. Owns the baked row bytes and a
/// bake-key → row-id map; the backend mirrors this into a wgpu
/// texture each frame by draining [`Self::flush`].
///
/// Row 0 is reserved as a magenta-fill fallback and never evicted.
/// Slots 1..ATLAS_ROWS are content-hashed and linear-probed. When the
/// table is full and the requested content isn't already resident, the
/// LRU row (smallest `last_used`) is evicted and re-baked in place —
/// excluding rows registered since the last flush, whose `LutRow` ids
/// this frame's draws already captured (see [`Self::lru_victim`]).
#[derive(Debug)]
pub(crate) struct CpuGradientAtlas {
    /// Exact bake key plus its probe hash per occupied row. Equality
    /// confirmation keeps a true hash collision from aliasing another
    /// gradient's baked row. Row 0 stays `None` because probing scans
    /// only `1..ATLAS_ROWS`; `baked[0]` carries the fallback payload.
    rows: [Option<GradientAtlasSlot>; ATLAS_ROWS as usize],
    /// Baked LUT row bytes, indexed by row id. Row 0's contents are
    /// the magenta-fallback fill. Storage is a single 512 KB heap
    /// allocation — `Vec<LutRowTexels>` is contiguous, so casting to
    /// `&[u8]` for the GPU upload is a free reinterpret.
    baked: Vec<LutRowTexels>,
    /// Per-row "last touched" timestamp. Bumped on every `register_stops`
    /// hit and on bake. The LRU victim is the row with the smallest
    /// stamp; row 0 is excluded. `u64` so wrap is unreachable in any
    /// realistic workload (a `u32` at 60 fps × 200 registers/frame
    /// rolls over in ~10 years and silently mis-evicts on wrap).
    last_used: [u64; ATLAS_ROWS as usize],
    /// Per-row: the [`Self::epoch`] the row was last registered in.
    /// [`Self::lru_victim`] refuses rows stamped with the *current*
    /// epoch — their `LutRow` ids are already captured in this frame's
    /// lowered draw payloads, so re-baking one would silently repaint
    /// those draws with the wrong gradient after the end-of-frame upload.
    row_epoch: [u64; ATLAS_ROWS as usize],
    /// Monotonic register counter. Each `register_stops` call bumps
    /// it and stamps the touched row, so within a single frame later
    /// registers are "newer" than earlier ones (fine — eviction needs
    /// a strict-order comparator, not wall-clock semantics).
    clock: u64,
    /// Current registration epoch, bumped once per [`Self::flush`] — the
    /// per-submit boundary. The atlas is shared across windows, but each
    /// window's submit re-registers its gradients before its own flush,
    /// so epoch-scoping eviction to "not registered since the last flush"
    /// is safe under cross-window interleaving (cross-frame eviction is
    /// harmless — the evictee re-bakes on its next register).
    epoch: u64,
    /// Contiguous row range changed since the last `flush`, widened on
    /// every bake; `None` when clean. The flush uploads `first..=last`
    /// in ONE `write_texture` (fixed API cost per call still dominates,
    /// so no per-row call list) — but range-sized: an animated gradient
    /// re-baking one row uploads 2 KB per frame, not the whole 512 KB
    /// atlas. Scattered dirty rows upload the min..=max span; that only
    /// approaches 512 KB when most of the atlas actually changed.
    dirty: Option<DirtyRows>,
}

/// Inclusive dirty row span for the next upload — see
/// [`CpuGradientAtlas::dirty`].
#[derive(Clone, Copy, Debug)]
struct DirtyRows {
    first: u32,
    last: u32,
}

/// One contiguous span of freshly baked LUT rows for GPU upload,
/// returned by [`CpuGradientAtlas::flush`]. `bytes` starts at row
/// `first_row` (the `write_texture` `origin.y`) and covers whole rows —
/// its length is a multiple of `size_of::<LutRowTexels>()`.
#[derive(Debug)]
pub(crate) struct FlushedRows<'a> {
    pub(crate) first_row: u32,
    pub(crate) bytes: &'a [u8],
}

impl Default for CpuGradientAtlas {
    fn default() -> Self {
        let mut atlas = Self {
            rows: std::array::from_fn(|_| None),
            baked: vec![[ColorF16::TRANSPARENT; LUT_ROW_TEXELS]; ATLAS_ROWS as usize],
            last_used: [0; ATLAS_ROWS as usize],
            row_epoch: [0; ATLAS_ROWS as usize],
            clock: 0,
            epoch: 0,
            dirty: None,
        };
        atlas.init_row_zero_magenta();
        atlas
    }
}

impl CpuGradientAtlas {
    /// Fill row 0 with bright magenta (sRGB `#ff00ff`, full alpha). Any
    /// quad whose `fill_lut_row = 0` paints this — visible at a glance,
    /// catches "registered with the atlas but the resulting row id
    /// didn't flow through to the quad."
    fn init_row_zero_magenta(&mut self) {
        // Linear (1, 0, 1, 1): the sRGB framebuffer encodes this to
        // #ff00ff on write, so the fallback reads as bright magenta.
        let magenta = ColorF16::from(Color::linear_rgba(1.0, 0.0, 1.0, 1.0));
        self.baked[0].fill(magenta);
        // No `rows[0]` sentinel: the probe range is `1..ATLAS_ROWS`,
        // so row 0 is unreachable regardless of what hash a real
        // gradient produces.
        //
        // First-frame upload paints the magenta fallback. (The other
        // rows start transparent-zero on the GPU too — wgpu textures
        // are zero-initialized — so uploading only row 0 is exact.)
        self.mark_row_dirty(0);
    }

    /// Find-or-bake the row for the gradient identified by `(stops,
    /// interp)`. Variant-agnostic: linear/radial/conic gradients with
    /// matching stops + interp share one row (the geometry differs in
    /// per-fragment `t`, but the LUT only depends on the colour-stop
    /// sequence). Returns the row id in `1..ATLAS_ROWS`.
    ///
    /// Bumps the per-row LRU stamp on every call so eviction picks the
    /// least-recently-touched row when the table is full.
    pub(crate) fn register_stops(&mut self, stops: &GradientStops, interp: Interp) -> LutRow {
        self.clock = self.clock.wrapping_add(1);
        let content_hash = hash_lut(stops, interp);
        // Probe starting at `1 + (hash mod 255)` so row 0 is never
        // claimed by a real gradient. Two passes: first look for a
        // match or an empty slot; if neither exists, evict the LRU
        // row (single linear scan over rows 1..ATLAS_ROWS).
        let base = (content_hash % (ATLAS_ROWS as u64 - 1)) as u32;
        for offset in 0..(ATLAS_ROWS - 1) {
            let row = 1 + (base + offset) % (ATLAS_ROWS - 1);
            match self.rows[row as usize].as_ref() {
                Some(slot)
                    if slot.content_hash == content_hash && slot.key.matches(stops, interp) =>
                {
                    // Hit: bump the LRU stamp and mark the row as
                    // referenced this epoch — its `LutRow` id is now in
                    // a draw payload, so it must not be evicted before
                    // the upload.
                    self.last_used[row as usize] = self.clock;
                    self.row_epoch[row as usize] = self.epoch;
                    return LutRow(row);
                }
                None => return self.claim_row(row, content_hash, stops, interp),
                _ => continue,
            }
        }
        // Atlas full: evict the LRU row not referenced this epoch. Row 0
        // (magenta fallback) is permanent — the scan starts at 1.
        let victim = self.lru_victim();
        self.claim_row(victim, content_hash, stops, interp)
    }

    /// Bake `(stops, interp)` into `row` and take over the slot: content
    /// hash, LRU stamp, epoch stamp, dirty-range widening. The one place
    /// a row's bookkeeping is written — shared by `register_stops`'
    /// empty-slot and evict arms so they can't drift.
    fn claim_row(
        &mut self,
        row: u32,
        content_hash: u64,
        stops: &GradientStops,
        interp: Interp,
    ) -> LutRow {
        bake_stops(stops, interp, &mut self.baked[row as usize]);
        self.rows[row as usize] = Some(GradientAtlasSlot {
            content_hash,
            key: GradientLutKey {
                stops: *stops,
                interp,
            },
        });
        self.last_used[row as usize] = self.clock;
        self.row_epoch[row as usize] = self.epoch;
        self.mark_row_dirty(row);
        LutRow(row)
    }

    /// Widen the pending dirty row range to include `row`.
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

    /// Scan rows 1..ATLAS_ROWS for the smallest `last_used` stamp among
    /// rows *not* registered in the current epoch — those rows' `LutRow`
    /// ids are already captured in this frame's draw payloads, so
    /// re-baking one would make those draws sample the wrong gradient
    /// after the end-of-frame upload. Always returns a row id ≥ 1 (row 0,
    /// the magenta fallback, is permanent).
    ///
    /// Panics when every row was registered this epoch: that's more
    /// distinct gradients in one frame than the atlas holds, and evicting
    /// any of them silently paints wrong colors — crash on the logic
    /// error instead.
    fn lru_victim(&self) -> u32 {
        let epoch = self.epoch;
        // 0 doubles as "no evictable row": row 0 is never a candidate.
        let mut best_row: u32 = 0;
        let mut best_stamp = u64::MAX;
        for row in 1..ATLAS_ROWS {
            if self.row_epoch[row as usize] == epoch {
                continue;
            }
            let s = self.last_used[row as usize];
            if s < best_stamp {
                best_stamp = s;
                best_row = row;
            }
        }
        assert!(
            best_row != 0,
            "gradient atlas exhausted: more than {} distinct gradients registered in \
             one frame — every LUT row is already referenced by this frame's draws, \
             so evicting any would silently paint the wrong gradient",
            ATLAS_ROWS - 1,
        );
        best_row
    }

    /// If any row changed since the last flush, return the contiguous
    /// dirty row span (see [`FlushedRows`]) for one-shot upload, and
    /// clear the dirty range. Returns `None` when nothing has changed —
    /// the steady-state idle frame uploads zero bytes.
    ///
    /// Also bumps the registration epoch: `flush` is the per-submit
    /// boundary, and rows registered since the previous flush are
    /// eviction-exempt until after this one (see [`Self::lru_victim`]).
    pub(crate) fn flush(&mut self) -> Option<FlushedRows<'_>> {
        self.epoch = self.epoch.wrapping_add(1);
        let dirty = self.dirty.take()?;
        let rows = &self.baked[dirty.first as usize..=dirty.last as usize];
        Some(FlushedRows {
            first_row: dirty.first,
            bytes: bytemuck::cast_slice(rows),
        })
    }
}

/// Content hash of the bake-relevant gradient inputs: the stop list
/// and the interpolation space. Stable across frames given identical
/// content; variant-agnostic so the same stops baked under the same
/// interp reuse one row regardless of geometry (linear angle, radial
/// centre/radius, conic centre/start-angle).
#[inline]
fn hash_lut(stops: &GradientStops, interp: Interp) -> u64 {
    let mut h = FxHasher::new();
    stops.hash(&mut h);
    interp.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests;
