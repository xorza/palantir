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

use crate::primitives::brush::gradient::Interp;
use crate::primitives::brush::gradient::stops::GradientStops;
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::fill_wire::LutRow;
use crate::renderer::gradient_atlas::bake::{LUT_ROW_TEXELS, LutRowTexels, bake_stops};
use crate::renderer::gradient_atlas::mru::MruList;
use crate::renderer::gradient_atlas::probe::GradientAtlasProbe;
use rustc_hash::FxHashMap;

#[cfg(feature = "bench")]
pub(crate) mod bench;

pub(crate) mod bake;
pub(crate) mod handle;
mod mru;
mod probe;

/// Rows the LUT atlas texture starts with. One row per distinct
/// gradient currently in use. Row 0 is reserved as a debug-magenta
/// fallback (so a `fill_lut_row = 0` from a bug paints obviously
/// wrong); real registrations occupy rows 1..capacity.
///
/// The atlas doubles from here (up to [`MAX_ATLAS_ROWS`], or the
/// device's `max_texture_dimension_2d` if that is lower) when one frame
/// registers more distinct gradients than fit — see
/// [`CpuGradientAtlas::grow`]. 256 rows is
/// 512 KB, and no realistic UI frame exceeds it, so growth is a
/// pathological-content escape hatch rather than a normal path.
pub(crate) const INITIAL_ATLAS_ROWS: u32 = 256;

/// Growth ceiling when no device limit is known (deviceless tests and
/// benches). 2048 is wgpu's downlevel `max_texture_dimension_2d`
/// floor, so it can't exceed a real adapter's cap.
pub(crate) const DEFAULT_MAX_ATLAS_ROWS: u32 = 2048;

/// Policy ceiling on rows, applied on top of whatever the device
/// allows.
///
/// The device's `max_texture_dimension_2d` (16384 on current discrete
/// parts) is a *hardware* bound; nothing about it reflects a judgement
/// on how many distinct gradients one frame should hold. Growth is a
/// one-way ratchet — the atlas never shrinks — so honouring the
/// hardware number lets a single pathological frame pin 32 MB of CPU
/// rows plus a 32 MB texture for the life of the process.
///
/// 4096 rows is 8 MB and still admits ~4000 distinct gradients in a
/// single frame. Past it [`LutRow::FALLBACK`] paints the debug magenta
/// row: loud and obviously wrong for the overflowing gradients, but it
/// neither crashes nor repaints what this frame's other draws captured.
pub(crate) const MAX_ATLAS_ROWS: u32 = 4096;

/// Exact bake identity shared by every gradient variant, and the key of
/// [`CpuGradientAtlas::index`].
///
/// `Hash` and `Eq` come from the fields: [`GradientStops`] hashes its
/// length and live stops and compares the same prefix, so equal keys
/// hash equal. A true 64-bit hash collision is resolved by the map's
/// own `Eq` check, not by anything this module has to write down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GradientLutKey {
    stops: GradientStops,
    interp: Interp,
}

/// CPU side of the gradient LUT atlas. Owns the baked row bytes and a
/// bake-key → row-id map; the backend mirrors this into a wgpu
/// texture each frame by draining [`Self::flush`].
///
/// Row 0 is reserved as a magenta-fill fallback and never evicted.
/// Lookup goes through [`Self::index`], which maps a bake key to
/// whichever row happens to hold it — the map never constrains *which*
/// row that is, so eviction and growth place rows freely. When no row
/// is free the least-recently-used one is evicted and re-baked in
/// place, excluding rows registered since the last flush, whose
/// `LutRow` ids this frame's draws already captured. When *every* row
/// is exempt the atlas [grows](Self::grow) instead, so a frame
/// authoring more distinct gradients than the table holds is valid
/// content rather than a failure.
///
/// ## Why the index is separate from the rows
///
/// One open-addressed table — hash the key, probe forward from that row
/// — cannot survive eviction here. A victim is chosen by recency, so
/// `claim_row` writes keys into rows at arbitrary distance from their
/// home slot, and the probe invariant ("reachable by walking forward
/// from home until an empty slot") dies at the first eviction. Keeping
/// lookups correct then requires the probe to scan *every* row before
/// giving up — a miss costing O(capacity), twice over once the LRU
/// search is counted, against a capacity that only ratchets upward.
/// Splitting the index from the rows makes every operation O(1) and, as
/// a side effect, lets [`Self::grow`] leave lookup completely alone:
/// resident gradients keep their rows and cannot be baked into a
/// second one.
#[derive(Debug)]
pub(crate) struct CpuGradientAtlas {
    /// Bake key → the row holding it. A pure lookup index: it says
    /// nothing about where a gradient *may* live, which is what lets
    /// eviction take the LRU row and growth append rows without either
    /// one disturbing it.
    index: FxHashMap<GradientLutKey, u32>,
    /// Row → the key it currently holds, so evicting a row can drop the
    /// outgoing gradient's index entry. `None` for a row never claimed.
    /// Row 0 stays `None` — it is not a member of the MRU list, so
    /// nothing can claim it; `baked[0]` carries the fallback payload.
    rows: Vec<Option<GradientLutKey>>,
    /// Baked LUT row bytes, indexed by row id. Row 0's contents are
    /// the magenta-fallback fill. Storage is a single heap allocation
    /// (512 KB at the initial capacity) — `Vec<LutRowTexels>` is
    /// contiguous, so casting to `&[u8]` for the GPU upload is a free
    /// reinterpret.
    baked: Vec<LutRowTexels>,
    /// Recency order over rows `1..capacity`. The list order *is* the
    /// recency order, so unlike `last_used` timestamps there is nothing
    /// to compare and no clock to overflow. See
    /// [`mru`] for why its tail alone answers "what may be evicted".
    mru: MruList,
    /// Per-row: the [`Self::epoch`] the row was last registered in.
    /// A row stamped with the *current* epoch cannot be evicted — its
    /// `LutRow` id is already captured in this frame's lowered draw
    /// payloads, so re-baking it would silently repaint those draws
    /// with the wrong gradient after the end-of-frame upload. `0` means
    /// never claimed, which is why [`Self::epoch`] starts at 1.
    row_epoch: Vec<u64>,
    /// Hard row ceiling: `min(device max_texture_dimension_2d,
    /// MAX_ATLAS_ROWS)`, since the atlas is one texture row per
    /// gradient. Registrations past a full table at this capacity paint
    /// the magenta fallback (see [`Self::register_stops`]).
    max_rows: u32,
    /// Current registration epoch, bumped once per [`Self::flush`] — the
    /// per-submit boundary. The atlas is shared across windows, but each
    /// window's submit re-registers its gradients before its own flush,
    /// so epoch-scoping eviction to "not registered since the last flush"
    /// is safe under cross-window interleaving (cross-frame eviction is
    /// harmless — the evictee re-bakes on its next register).
    ///
    /// Starts at 1 so a never-claimed row's `row_epoch` of 0 can never
    /// read as epoch-current.
    epoch: u64,
    /// How each registration resolved — see [`GradientAtlasProbe`].
    /// Zero-sized in a shipping build.
    probe: GradientAtlasProbe,
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
    /// The atlas's current row count. The backend recreates its texture
    /// when this differs from the live one — [`CpuGradientAtlas::grow`]
    /// dirties every row, so the upload that reports a new height also
    /// refills the replacement texture.
    pub(crate) total_rows: u32,
}

impl Default for CpuGradientAtlas {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ATLAS_ROWS)
    }
}

impl CpuGradientAtlas {
    /// Atlas capped at `max_rows` rows. Starts at
    /// [`INITIAL_ATLAS_ROWS`]; a cap below that would make the initial
    /// allocation itself illegal, so it is raised to fit.
    pub(crate) fn new(max_rows: u32) -> Self {
        let mut atlas = Self {
            index: FxHashMap::default(),
            rows: vec![None; INITIAL_ATLAS_ROWS as usize],
            baked: vec![[ColorF16::TRANSPARENT; LUT_ROW_TEXELS]; INITIAL_ATLAS_ROWS as usize],
            mru: MruList::seeded(INITIAL_ATLAS_ROWS),
            row_epoch: vec![0; INITIAL_ATLAS_ROWS as usize],
            max_rows: max_rows.max(INITIAL_ATLAS_ROWS),
            epoch: 1,
            dirty: None,
            probe: GradientAtlasProbe::default(),
        };
        atlas.init_row_zero_magenta();
        atlas
    }

    /// Rows currently allocated, including the reserved row 0. The
    /// per-row columns are resized together in [`Self::grow`], so
    /// `baked` speaks for all of them.
    pub(crate) fn capacity(&self) -> u32 {
        self.baked.len() as u32
    }

    /// Fill row 0 with bright magenta (sRGB `#ff00ff`, full alpha). Any
    /// quad whose `fill_lut_row = 0` paints this — visible at a glance,
    /// catches "registered with the atlas but the resulting row id
    /// didn't flow through to the quad."
    fn init_row_zero_magenta(&mut self) {
        // Linear (1, 0, 1, 1): the sRGB framebuffer encodes this to
        // #ff00ff on write, so the fallback reads as bright magenta.
        let magenta = ColorF16::from(Color::linear_rgba(1.0, 0.0, 1.0, 1.0));
        self.baked[0].fill(magenta);
        // No `rows[0]` sentinel: row 0 is not a member of the MRU list,
        // so no claim can ever select it.
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
    /// sequence). Returns the row id in `1..capacity`.
    ///
    /// Moves the row to the MRU head on every call — hit or claim — so
    /// eviction can read the least-recently-registered row off the tail
    /// (see [`mru`]).
    ///
    /// Three escalating arms once every row is occupied: claim the
    /// tail if it isn't referenced this epoch, else [grow](Self::grow)
    /// the atlas, else — only at [`MAX_ATLAS_ROWS`] or the device's
    /// texture-height cap, whichever is lower — return
    /// [`LutRow::FALLBACK`]. That last arm paints the debug magenta
    /// row: loudly wrong for the overflowing gradients, but it neither
    /// crashes nor corrupts the rows this frame's other draws already
    /// captured.
    pub(crate) fn register_stops(&mut self, stops: &GradientStops, interp: Interp) -> LutRow {
        self.probe.registration();
        let key = GradientLutKey {
            stops: *stops,
            interp,
        };
        // Hit: one map probe, then mark the row as referenced this epoch
        // — its `LutRow` id is now in a draw payload, so it must not be
        // evicted before the upload.
        if let Some(&row) = self.index.get(&key) {
            self.probe.hit();
            self.touch(row);
            return LutRow(row);
        }
        loop {
            // The MRU tail is either a never-claimed row or the
            // least-recently-registered one, and epoch-current rows form
            // a head prefix (see `mru`), so this single check is the
            // whole eviction decision.
            let victim = self.mru.tail();
            if self.row_epoch[victim as usize] != self.epoch {
                return self.claim_row(victim, key);
            }
            // Every row is spoken for by this frame's draws. Grow and
            // retry: the grown rows land at the tail unclaimed, so the
            // next pass takes one and this loop runs at most twice.
            if !self.grow() {
                self.probe.fallback();
                return LutRow::FALLBACK;
            }
        }
    }

    /// Mark `row` as the most recently used and referenced this epoch.
    /// Every registration path goes through here — that is what makes
    /// the epoch-current rows a head prefix of the MRU list, which is
    /// what lets [`Self::register_stops`] decide eviction from the tail
    /// alone. Pinned by `epoch_current_rows_form_an_mru_prefix`.
    #[inline]
    fn touch(&mut self, row: u32) {
        self.mru.touch(row);
        self.row_epoch[row as usize] = self.epoch;
    }

    /// Double the row count (capped at [`Self::max_rows`]), reporting
    /// whether the atlas actually grew.
    ///
    /// Resident rows keep their ids — they must, since this frame's
    /// draw payloads already hold them — and because [`Self::index`]
    /// maps keys to rows directly, growth doesn't perturb lookup at
    /// all: a resident gradient still resolves to its original row.
    /// The new rows join the MRU list at the eviction end, so they are
    /// claimed before any resident row is evicted.
    fn grow(&mut self) -> bool {
        let capacity = self.capacity();
        let grown = capacity.saturating_mul(2).min(self.max_rows);
        if grown <= capacity {
            return false;
        }
        self.rows.resize(grown as usize, None);
        self.baked
            .resize(grown as usize, [ColorF16::TRANSPARENT; LUT_ROW_TEXELS]);
        self.row_epoch.resize(grown as usize, 0);
        self.mru.extend_to(capacity, grown);
        self.probe.growth();
        // The backend replaces its texture at the new height and wgpu
        // zero-initializes the replacement, so every row — not just the
        // new ones — has to re-upload.
        self.dirty = Some(DirtyRows {
            first: 0,
            last: grown - 1,
        });
        // Independent resizes make equal lengths a convention rather
        // than a type invariant, and `capacity` reads only `baked`. Cold
        // path — growth happens at most once per doubling — so the MRU
        // walk rides along.
        debug_assert!(
            self.rows.len() == self.baked.len() && self.row_epoch.len() == self.baked.len(),
            "per-row columns must resize together",
        );
        debug_assert!(self.mru.is_well_formed(), "growth corrupted the MRU list",);
        true
    }

    /// Bake `key` into `row` and take over the slot: index entry,
    /// recency, epoch stamp, dirty-range widening. The one place a
    /// row's bookkeeping is written — shared by `register_stops`'
    /// free-row and evict arms so they can't drift.
    fn claim_row(&mut self, row: u32, key: GradientLutKey) -> LutRow {
        debug_assert_ne!(row, 0, "row 0 is the permanent magenta fallback");
        // Evicting: the outgoing gradient's index entry has to go with
        // its row, or a later lookup resolves to a row now holding
        // somebody else's bake.
        let displaced = self.rows[row as usize].replace(key);
        if let Some(evicted) = displaced {
            self.index.remove(&evicted);
        }
        self.probe.bake(displaced.is_some());
        bake_stops(&key.stops, key.interp, &mut self.baked[row as usize]);
        self.index.insert(key, row);
        self.touch(row);
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

    /// If any row changed since the last flush, return the contiguous
    /// dirty row span (see [`FlushedRows`]) for one-shot upload, and
    /// clear the dirty range. Returns `None` when nothing has changed —
    /// the steady-state idle frame uploads zero bytes.
    ///
    /// Also bumps the registration epoch: `flush` is the per-submit
    /// boundary, and rows registered since the previous flush are
    /// eviction-exempt until after this one (see [`Self::register_stops`]).
    pub(crate) fn flush(&mut self) -> Option<FlushedRows<'_>> {
        self.epoch = self.epoch.wrapping_add(1);
        let dirty = self.dirty.take()?;
        let total_rows = self.capacity();
        let rows = &self.baked[dirty.first as usize..=dirty.last as usize];
        Some(FlushedRows {
            first_row: dirty.first,
            bytes: bytemuck::cast_slice(rows),
            total_rows,
        })
    }
}

#[cfg(test)]
mod internals {
    use super::*;

    impl CpuGradientAtlas {
        /// Whether the rows registered this epoch form a head prefix of
        /// the MRU list — the property [`Self::register_stops`] decides
        /// eviction from, by checking the tail alone. Every registration
        /// path must move its row to the head; one that stamps
        /// `row_epoch` without doing so would leave an evictable-looking
        /// row ahead of a protected one, and the atlas would repaint a
        /// row this frame's draws already reference.
        pub(crate) fn epoch_prefix_holds(&self) -> bool {
            let mut seen_stale = false;
            for row in self.mru.to_vec() {
                match self.row_epoch[row as usize] == self.epoch {
                    true if seen_stale => return false,
                    true => {}
                    false => seen_stale = true,
                }
            }
            true
        }

        /// The row `key`'s gradient currently occupies, straight out of
        /// the index — so a test can tell "resolved to the same row"
        /// from "re-baked into a new one" without registering (which
        /// would itself move the row).
        /// Live index entries — one per occupied row, so a duplicate
        /// bake or a leaked eviction entry shows up as a mismatch
        /// against the rows actually claimed.
        pub(crate) fn index_len(&self) -> usize {
            self.index.len()
        }

        pub(crate) fn max_rows(&self) -> u32 {
            self.max_rows
        }

        pub(crate) fn resident_row(&self, stops: &GradientStops, interp: Interp) -> Option<u32> {
            self.index
                .get(&GradientLutKey {
                    stops: *stops,
                    interp,
                })
                .copied()
        }
    }
}

#[cfg(test)]
mod tests;
