//! CPU side of the gradient LUT atlas. Bakes stop sequences into LUT
//! rows shared across linear / radial / conic gradient variants; the
//! shader does the per-fragment `t` derivation. See [`bake_stops`] and
//! [`GradientCpuAtlas::register_stops`].
//!
//! ## Bake output convention
//!
//! Each baked row is 256 [`ColorF16`] texels = 2048 bytes, **straight
//! (non-premultiplied) linear-RGB** f16. The backend uploads these into
//! an `Rgba16Float` texture (no auto-decode); the shader samples and
//! gets the stored linear value directly as f16-decoded floats.
//! Premultiply happens in the shader on the sampled value — same
//! convention as the rest of the pipeline (see "Colour pipeline" in
//! `CLAUDE.md`).
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

use crate::animation::animatable::Animatable;
use crate::common::hash::Hasher as FxHasher;
use crate::primitives::brush::{Interp, MAX_STOPS, Stop};
use crate::primitives::color::{Color, ColorF16, linear_to_oklab, oklab_to_linear};
use crate::primitives::fill_wire::LutRow;
use std::cell::RefCell;
use std::hash::Hasher;
use std::rc::Rc;

/// Number of rows in the LUT atlas texture. One row per distinct
/// gradient currently in use. Row 0 is reserved as a debug-magenta
/// fallback (so a `fill_lut_row = 0` from a bug paints obviously
/// wrong); real registrations occupy rows 1..ATLAS_ROWS.
pub(crate) const ATLAS_ROWS: u32 = 256;

/// Width of one baked row in texels. Picked to match the LUT texture's
/// 256-texel width; 256 gives 1 LSB per stride on the parametric axis,
/// well below 8-bit display precision.
pub(crate) const LUT_ROW_TEXELS: usize = 256;

/// One baked LUT row: 256 straight-alpha linear-RGB f16 texels
/// (`ColorF16`, 8 bytes each → 2048 bytes/row). Contiguous and `Pod`,
/// so the atlas casts `&[LutRowTexels]` straight to `&[u8]` for upload.
pub(crate) type LutRowTexels = [ColorF16; LUT_ROW_TEXELS];

/// Bake a [`LinearGradient`] into a single LUT row.
///
/// Output is straight (non-premultiplied) linear-RGB f16 — see module
/// docs. `out` is a `&mut LutRowTexels`, written in place; the buffer is
/// fully overwritten, no read-before-write.
///
/// Edge clamp: `t < first_stop.offset()` paints `first_stop.color`;
/// `t > last_stop.offset()` paints `last_stop.color`. Spread modes
/// (`Pad`/`Repeat`/`Reflect`) are applied **shader-side** on the
/// sampling `t` coordinate, not at bake time — one row serves all
/// spread modes for the same gradient.
pub(crate) fn bake_stops(stops: &[Stop], interp: Interp, out: &mut LutRowTexels) {
    // Caller invariant: 2..=MAX_STOPS. Asserted so accidental callers
    // from elsewhere in the crate trip immediately instead of UB-ing
    // through `lerp_at`'s `stops[len-1]` / bracket reads.
    assert!(
        (2..=MAX_STOPS).contains(&stops.len()),
        "bake_stops requires 2..={MAX_STOPS} stops, got {}",
        stops.len(),
    );
    // Sort stops by offset into a stack scratch. 8 elements max, simple
    // insertion sort beats any allocation. Equal offsets stay in input
    // order (stable) so a hard-transition pair (`(0.5, A), (0.5, B)`)
    // picks `A` on the left, `B` on the right.
    let mut sorted: [Stop; MAX_STOPS] = Default::default();
    let n = stops.len();
    sorted[..n].copy_from_slice(stops);
    for i in 1..n {
        let mut j = i;
        while j > 0 && sorted[j - 1].offset() > sorted[j].offset() {
            sorted.swap(j - 1, j);
            j -= 1;
        }
    }

    // Precompute the per-stop forms the inner loop wants — once for
    // the row, not once per texel. Linear `Color` is needed by every
    // interp (both `Linear` and `Oklab` math run in linear space).
    // `Oklab` additionally needs the L/a/b triplet per stop —
    // computing it 256× per texel would be wasted.
    let mut linear_stops: [Color; MAX_STOPS] = [Color::TRANSPARENT; MAX_STOPS];
    for i in 0..n {
        linear_stops[i] = sorted[i].color.into();
    }
    let mut oklab_stops: [[f32; 3]; MAX_STOPS] = [[0.0; 3]; MAX_STOPS];
    if matches!(interp, Interp::Oklab) {
        for i in 0..n {
            let c = linear_stops[i];
            oklab_stops[i] = linear_to_oklab(c.r, c.g, c.b);
        }
    }

    for (i, texel) in out.iter_mut().enumerate() {
        let t = i as f32 / (LUT_ROW_TEXELS - 1) as f32;
        let color = lerp_at(
            &sorted[..n],
            &linear_stops[..n],
            &oklab_stops[..n],
            t,
            interp,
        );
        *texel = ColorF16::from(color);
    }
}

/// Resolve the colour at parametric `t ∈ 0..=1`. Edge clamp outside the
/// first/last stop offsets; bracket-and-lerp in between. `linear` and
/// `oklab` are precomputed parallel arrays — see [`bake_stops`] — so
/// every texel reads pre-decoded forms and never re-runs the cubic
/// or Oklab decode.
#[inline]
fn lerp_at(stops: &[Stop], linear: &[Color], oklab: &[[f32; 3]], t: f32, interp: Interp) -> Color {
    if t <= stops[0].offset() {
        return linear[0];
    }
    if t >= stops[stops.len() - 1].offset() {
        return linear[stops.len() - 1];
    }
    let mut i = 1;
    while i < stops.len() && stops[i].offset() < t {
        i += 1;
    }
    let a_off = stops[i - 1].offset();
    let b_off = stops[i].offset();
    let denom = b_off - a_off;
    // Equal-offset hard transition: pick the right-hand stop.
    let u = if denom.abs() <= f32::EPSILON {
        return linear[i];
    } else {
        (t - a_off) / denom
    };

    let ca = linear[i - 1];
    let cb = linear[i];
    match interp {
        // `Color` stores linear-RGB, so its `Animatable` lerp *is* the
        // linear-space blend; f16 quantization happens once at the
        // `ColorF16::from` in `bake_stops`.
        Interp::Linear => Color::lerp(ca, cb, u),
        Interp::Oklab => lerp_oklab(ca, cb, oklab[i - 1], oklab[i], u),
    }
}

/// Lerp in Oklab. `lab_a` / `lab_b` are precomputed from
/// `linear_to_oklab` in `bake_stops`; this only runs the three lerps
/// and the inverse Oklab transform per texel.
#[inline]
fn lerp_oklab(ca: Color, cb: Color, lab_a: [f32; 3], lab_b: [f32; 3], u: f32) -> Color {
    let lab = [
        lab_a[0] + (lab_b[0] - lab_a[0]) * u,
        lab_a[1] + (lab_b[1] - lab_a[1]) * u,
        lab_a[2] + (lab_b[2] - lab_a[2]) * u,
    ];
    let rgb = oklab_to_linear(lab);
    Color {
        r: rgb[0],
        g: rgb[1],
        b: rgb[2],
        // Qualified: bare `ca.a.lerp(..)` would hit std's unstable
        // inherent `f32::lerp` and fail to compile.
        a: <f32 as Animatable>::lerp(ca.a, cb.a, u),
    }
}

/// CPU side of the gradient LUT atlas. Owns the baked row bytes and a
/// content-hash → row-id map; the backend mirrors this into a wgpu
/// texture each frame by draining [`Self::flush`].
///
/// Row 0 is reserved as a magenta-fill fallback and never evicted.
/// Slots 1..ATLAS_ROWS are content-hashed and linear-probed. When the
/// table is full and the requested content isn't already resident, the
/// LRU row (smallest `last_used`) is evicted and re-baked in place —
/// excluding rows registered since the last flush, whose `LutRow` ids
/// this frame's draws already captured (see [`Self::lru_victim`]).
#[derive(Debug)]
pub(crate) struct GradientCpuAtlas {
    /// `Some(content_hash)` per row occupied by a gradient; `None` for
    /// free rows. Row 0 is unreachable from the probe (which scans
    /// `1..ATLAS_ROWS`) so its slot stays `None` — the magenta-fill
    /// payload in `baked[0]` is the real fallback contract.
    rows: [Option<u64>; ATLAS_ROWS as usize],
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
/// [`GradientCpuAtlas::dirty`].
#[derive(Clone, Copy, Debug)]
struct DirtyRows {
    first: u32,
    last: u32,
}

/// One contiguous span of freshly baked LUT rows for GPU upload,
/// returned by [`GradientCpuAtlas::flush`]. `bytes` starts at row
/// `first_row` (the `write_texture` `origin.y`) and covers whole rows —
/// its length is a multiple of `size_of::<LutRowTexels>()`.
#[derive(Debug)]
pub(crate) struct FlushedRows<'a> {
    pub(crate) first_row: u32,
    pub(crate) bytes: &'a [u8],
}

impl Default for GradientCpuAtlas {
    fn default() -> Self {
        let mut atlas = Self {
            rows: [None; ATLAS_ROWS as usize],
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

impl GradientCpuAtlas {
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
    pub(crate) fn register_stops(&mut self, stops: &[Stop], interp: Interp) -> LutRow {
        self.clock = self.clock.wrapping_add(1);
        let content_hash = hash_stops(stops, interp);
        // Probe starting at `1 + (hash mod 255)` so row 0 is never
        // claimed by a real gradient. Two passes: first look for a
        // match or an empty slot; if neither exists, evict the LRU
        // row (single linear scan over rows 1..ATLAS_ROWS).
        let base = (content_hash % (ATLAS_ROWS as u64 - 1)) as u32;
        for offset in 0..(ATLAS_ROWS - 1) {
            let row = 1 + (base + offset) % (ATLAS_ROWS - 1);
            match self.rows[row as usize] {
                Some(h) if h == content_hash => {
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
    fn claim_row(&mut self, row: u32, content_hash: u64, stops: &[Stop], interp: Interp) -> LutRow {
        bake_stops(stops, interp, &mut self.baked[row as usize]);
        self.rows[row as usize] = Some(content_hash);
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

/// Cross-frame shared handle for the gradient LUT atlas. Cheap to
/// clone (Rc-shared); `WindowRenderer` owns the canonical instance and hands
/// clones to subsystems that register or flush gradients. Sibling of
/// [`crate::ImageRegistry`] — same lifetime, same access pattern.
#[derive(Clone, Debug, Default)]
pub(crate) struct GradientAtlas {
    inner: Rc<RefCell<GradientCpuAtlas>>,
}

impl GradientAtlas {
    /// Find-or-bake the row for `(stops, interp)`. See
    /// [`GradientCpuAtlas::register_stops`].
    #[inline]
    pub(crate) fn register_stops(&self, stops: &[Stop], interp: Interp) -> LutRow {
        self.inner.borrow_mut().register_stops(stops, interp)
    }

    /// Run `f` on the flushed dirty row span if the atlas is dirty,
    /// returning `f`'s result. `None` when nothing changed (steady-state
    /// idle frame). Closure form so the underlying borrow stays scoped to
    /// the call — backend's `GradientResources::upload` runs
    /// `queue.write_texture` inside the closure.
    #[inline]
    pub(crate) fn flush_with<R>(&self, f: impl FnOnce(FlushedRows<'_>) -> R) -> Option<R> {
        let mut atlas = self.inner.borrow_mut();
        atlas.flush().map(f)
    }
}

/// Content hash of the bake-relevant gradient inputs: the stop list
/// and the interpolation space. Stable across frames given identical
/// content; variant-agnostic so the same stops baked under the same
/// interp reuse one row regardless of geometry (linear angle, radial
/// centre/radius, conic centre/start-angle).
#[inline]
fn hash_stops(stops: &[Stop], interp: Interp) -> u64 {
    let mut h = FxHasher::new();
    h.write_u16(((interp as u16) << 8) | (stops.len() as u16));
    for s in stops {
        // Pack `(color_u32, offset_u8)` into one u64 — one hasher
        // write per stop instead of five. `offset` is already u8
        // quantized; no `canon_bits` needed (no NaN / -0 to canonicalise).
        let packed = ((s.color.to_u32() as u64) << 32) | (s.offset_u8 as u64);
        h.write_u64(packed);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use crate::primitives::brush::LinearGradient;
    use crate::primitives::color::ColorU8;
    use crate::renderer::gradient_atlas::*;
    use std::collections::HashSet;

    /// One baked texel decoded back to a linear `Color`. The f16 store
    /// round-trips losslessly enough that `≈` comparisons hold to well
    /// under a u8 LSB (1/255).
    fn texel(out: &LutRowTexels, i: usize) -> Color {
        out[i].unpack()
    }

    /// Expected linear value of a `ColorU8` channel: `ColorU8` is linear
    /// storage, so the stored byte / 255 *is* the linear value the bake
    /// interpolates between (no sRGB decode).
    fn lin(byte: u8) -> f32 {
        byte as f32 / 255.0
    }

    /// Fresh f16 LUT row, all texels transparent before bake.
    fn fresh_row() -> LutRowTexels {
        [ColorF16::TRANSPARENT; LUT_ROW_TEXELS]
    }

    /// `Interp::Linear`: midpoint of black→white in linear-RGB space
    /// is exactly linear 0.5. The sampler reads the f16 store directly
    /// as the linear value the shader uses. Regression check: an
    /// accidental sRGB-space lerp would produce linear ≈ 0.215, far
    /// below the 0.4 threshold.
    #[test]
    fn linear_midpoint_black_to_white_is_half() {
        let g = LinearGradient::two_stop(0.0, ColorU8::BLACK, ColorU8::WHITE)
            .with_interp(Interp::Linear);
        let mut out = fresh_row();
        bake_stops(&g.stops, g.interp, &mut out);
        let mid = texel(&out, 127);
        assert!(
            (0.4..=0.6).contains(&mid.r),
            "linear-RGB midpoint should be near linear 0.5, got {}",
            mid.r,
        );
        assert_eq!(mid.r, mid.g);
        assert_eq!(mid.g, mid.b);
        assert_eq!(mid.a, 1.0);
    }

    /// `Interp::Oklab`: red→green midpoint should *not* be muddy
    /// brown (which is what linear-RGB lerps produce). Specifically,
    /// the green channel at midpoint should be high (Oklab keeps
    /// luminance up through the midpoint by traversing yellow-ish
    /// hues rather than dipping through dark brown).
    #[test]
    fn oklab_red_to_green_midpoint_avoids_muddy_brown() {
        let red = ColorU8::rgb(255, 0, 0);
        let green = ColorU8::rgb(0, 255, 0);
        let g = LinearGradient::two_stop(0.0, red, green).with_interp(Interp::Oklab);
        let mut out = fresh_row();
        bake_stops(&g.stops, g.interp, &mut out);
        let mid = texel(&out, 127);
        // Both channels should be non-trivial at midpoint — Oklab
        // hits a yellowish midpoint, not the dark muddy brown that
        // linear-RGB lerp produces. The f16 store holds linear values
        // directly; expect high red (>0.47 ≈ 120/255) and moderate
        // green (>0.31 ≈ 80/255) reflecting the warm-yellow midpoint.
        assert!(
            mid.r > 0.47 && mid.g > 0.31,
            "Oklab red→green midpoint should preserve luminance; got ({}, {}, {})",
            mid.r,
            mid.g,
            mid.b,
        );
    }

    /// First and last texels match the corresponding stop colours
    /// exactly. Catches off-by-one in the parametric `t = i/(N-1)`
    /// stride and the edge-clamp guard.
    #[test]
    fn endpoints_match_stops_exactly() {
        let c0 = ColorU8::rgb(11, 22, 33);
        let c1 = ColorU8::rgb(244, 233, 222);
        for interp in [Interp::Linear, Interp::Oklab] {
            let g = LinearGradient::two_stop(0.0, c0, c1).with_interp(interp);
            let mut out = fresh_row();
            bake_stops(&g.stops, g.interp, &mut out);
            let first = texel(&out, 0);
            let last = texel(&out, LUT_ROW_TEXELS - 1);
            // Endpoints are an exact edge-clamp to the stop's linear
            // value; the only loss is the f16 quantize, well under a
            // u8 LSB (1/255 ≈ 0.004).
            let tol = 1.0 / 255.0;
            for (chan, (got, want)) in [
                (first.r, lin(c0.r)),
                (first.g, lin(c0.g)),
                (first.b, lin(c0.b)),
                (last.r, lin(c1.r)),
                (last.g, lin(c1.g)),
                (last.b, lin(c1.b)),
            ]
            .into_iter()
            .enumerate()
            {
                assert!(
                    (got - want).abs() <= tol,
                    "interp={interp:?} chan {chan}: got {got} want {want}",
                );
            }
        }
    }

    /// 3-stop gradient at offset `0.25` falls in the first half of the
    /// `0.0..0.5` bracket — should be halfway between stop 0 and stop
    /// 1, not stop 1 and stop 2. Catches bracketing logic.
    #[test]
    fn three_stop_quarter_brackets_first_pair() {
        let g = LinearGradient::three_stop(
            0.0,
            ColorU8::rgb(0, 0, 0),   // stop at 0.0
            ColorU8::rgb(255, 0, 0), // stop at 0.5
            ColorU8::rgb(0, 0, 255), // stop at 1.0
        )
        .with_interp(Interp::Linear);
        let mut out = fresh_row();
        bake_stops(&g.stops, g.interp, &mut out);
        // Texel at i=64 ≈ t=0.251 → halfway between stops 0 and 1.
        // r channel: lerp(0.0, 1.0, 0.502) ≈ 0.502.
        let q = texel(&out, 64);
        assert!(
            (q.r - 0.502).abs() <= 0.01,
            "quarter-texel r={} not ~0.502 (bracketing first pair)",
            q.r,
        );
        // b should still be near 0 (stop 2's b=1.0 isn't reached yet).
        assert!(q.b <= 0.02, "quarter-texel b={} leaked from stop 2", q.b);
    }

    /// Pin the row layout: 256 `ColorF16` texels = 2048 bytes total,
    /// `[r, g, b, a]` f16 lanes per texel. Endpoint texels decode back
    /// to their stops' linear values.
    #[test]
    fn lut_row_layout() {
        assert_eq!(LUT_ROW_TEXELS, 256);
        assert_eq!(size_of::<LutRowTexels>(), 2048);
        assert_eq!(size_of::<ColorF16>(), 8);
        let g = LinearGradient::two_stop(0.0, ColorU8::rgb(1, 2, 3), ColorU8::rgb(4, 5, 6));
        let mut out = fresh_row();
        bake_stops(&g.stops, g.interp, &mut out);
        let tol = 1.0 / 255.0;
        let approx = |got: f32, want: f32| assert!((got - want).abs() <= tol, "{got} vs {want}");
        let first = texel(&out, 0);
        approx(first.r, lin(1));
        approx(first.g, lin(2));
        approx(first.b, lin(3));
        assert_eq!(first.a, 1.0);
        let last = texel(&out, LUT_ROW_TEXELS - 1);
        approx(last.r, lin(4));
        approx(last.g, lin(5));
        approx(last.b, lin(6));
        assert_eq!(last.a, 1.0);
    }

    /// Unsorted stops are sorted at bake time. Authors shouldn't rely
    /// on this — `LinearGradient::new` accepts any order — but the
    /// bake must produce a sensible output regardless.
    #[test]
    fn unsorted_stops_get_sorted_at_bake() {
        let stops = [
            Stop::new(1.0, ColorU8::rgb(255, 0, 0)), // out of order
            Stop::new(0.0, ColorU8::rgb(0, 0, 255)),
        ];
        let g = LinearGradient::new(0.0, stops);
        let mut out = fresh_row();
        bake_stops(&g.stops, g.interp, &mut out);
        // First texel should be blue (the stop at 0.0), last should be red.
        let first = texel(&out, 0);
        let last = texel(&out, LUT_ROW_TEXELS - 1);
        assert_eq!((first.r, first.g, first.b), (0.0, 0.0, 1.0));
        assert_eq!((last.r, last.g, last.b), (1.0, 0.0, 0.0));
    }

    /// Stops covering only `0.25..0.75` clamp at the edges: texels
    /// before 0.25 paint the first stop's colour, after 0.75 paint
    /// the last stop's colour. Spread modes (Pad/Repeat/Reflect) are
    /// applied later in the shader on `t`, not here; the bake just
    /// emits the parametric range with edge-clamp behaviour.
    #[test]
    fn partial_range_clamps_at_edges() {
        let stops = [
            Stop::new(0.25, ColorU8::rgb(0, 255, 0)),
            Stop::new(0.75, ColorU8::rgb(0, 0, 255)),
        ];
        let g = LinearGradient::new(0.0, stops);
        let mut out = fresh_row();
        bake_stops(&g.stops, g.interp, &mut out);
        // Texel 0 (t=0): clamped to first stop colour (green).
        assert_eq!(texel(&out, 0).g, 1.0);
        // Texel 255 (t=1): clamped to last stop colour (blue).
        assert_eq!(texel(&out, LUT_ROW_TEXELS - 1).b, 1.0);
    }

    /// The showcase's dark `#1a1a2e → #4c5cdb` gradient is the
    /// motivating case for the f16 store. Both stops linearise to tiny
    /// reds (3/255 → 19/255), so an 8-bit *linear* row crushes the
    /// red channel onto ~16 integer steps across 256 texels — the
    /// visible banding. The f16 row keeps a distinct value at nearly
    /// every texel. This asserts both sides: the f16 row is smooth,
    /// and re-quantizing the same reds to 8-bit linear reproduces the
    /// banding (so the test fails loudly if the premise ever changes).
    #[test]
    fn dark_gradient_row_has_no_banding() {
        let navy = ColorU8::hex(0x1a1a2e);
        let blue = ColorU8::hex(0x4c5cdb);
        // The whole problem: both stops linearise to tiny reds (≈ 2/255
        // and 18/255), so the bake walks a narrow span that an 8-bit
        // linear row can't resolve. Bounded, not exact-pinned, so a
        // tweak to the sRGB cubic fit doesn't break this test.
        assert!(
            navy.r < 6 && blue.r < 24,
            "stops not dark: navy.r={} blue.r={}",
            navy.r,
            blue.r
        );
        let g = LinearGradient::two_stop(0.0, navy, blue); // default Oklab
        let mut out = fresh_row();
        bake_stops(&g.stops, g.interp, &mut out);

        let reds: Vec<f32> = (0..LUT_ROW_TEXELS).map(|i| texel(&out, i).r).collect();

        // f16 store: per-texel red delta (~2.5e-4) dwarfs the f16 ulp
        // (~8e-6) at this magnitude, so distinct reds ≈ texel count.
        let distinct_f16 = reds
            .iter()
            .map(|r| r.to_bits())
            .collect::<HashSet<_>>()
            .len();
        assert!(
            distinct_f16 >= 180,
            "f16 red banded: only {distinct_f16} distinct levels"
        );

        // Counterfactual: the old `Rgba8Unorm` store quantized these
        // same reds to 8-bit linear, collapsing onto ≤ 20 levels.
        let distinct_u8 = reds
            .iter()
            .map(|r| (r * 255.0).round() as u8)
            .collect::<HashSet<_>>()
            .len();
        assert!(
            distinct_u8 <= 20,
            "premise check: 8-bit linear should band hard, got {distinct_u8} levels",
        );
    }

    /// Vary the *stops* (the only thing the row key now depends on)
    /// across calls. Geometry (angle/centre/etc.) is now atlas-key
    /// irrelevant — varying angle would silently produce row reuse
    /// under the (stops, interp) keying.
    fn distinct_grad(seed: f32) -> LinearGradient {
        // FxHash on the seed bits gives well-distributed 32-bit chunks
        // for the (r, g, b) bytes, so different seeds produce visibly
        // different stop colours and the (stops, interp) hash lands in
        // distinct atlas rows.
        let mut h = FxHasher::new();
        h.write_u32(seed.to_bits());
        let v = h.finish();
        let r = v as u8;
        let g = (v >> 8) as u8;
        let b = (v >> 16) as u8;
        LinearGradient::two_stop(0.0, ColorU8::rgb(r, g, b), ColorU8::rgb(0, 0xff, 0))
    }

    fn register_for(atlas: &mut GradientCpuAtlas, g: LinearGradient) -> LutRow {
        atlas.register_stops(&g.stops, g.interp)
    }

    fn assert_real_row(row: LutRow) {
        assert!(
            (1..ATLAS_ROWS).contains(&row.0),
            "row {} must be in 1..ATLAS_ROWS",
            row.0,
        );
    }

    /// Row 0 is reserved magenta. Created at construction; dirty list
    /// flags it so the first frame's GPU upload paints the fallback row.
    /// First real registration goes to row 1 (or wherever its hash lands
    /// in 1..ATLAS_ROWS).
    #[test]
    fn row_zero_reserved_as_magenta_fallback() {
        let atlas = GradientCpuAtlas::default();
        // Row 0 is linear (1, 0, 1, 1) across all texels — encodes to
        // #ff00ff on the sRGB framebuffer.
        let magenta = ColorF16::from(Color::linear_rgba(1.0, 0.0, 1.0, 1.0));
        assert!(atlas.baked[0].iter().all(|&t| t == magenta));
    }

    /// First real `register` goes through the probe path. The atlas
    /// is already dirty from magenta init; registering should keep it
    /// dirty so the GPU upload includes the new row.
    #[test]
    fn register_returns_nonzero_row_and_marks_dirty() {
        let mut atlas = GradientCpuAtlas::default();
        let g = distinct_grad(0.1);
        let row = atlas.register_stops(&g.stops, g.interp);
        assert_real_row(row);
        assert!(atlas.dirty.is_some(), "register must mark atlas dirty");
    }

    /// Same gradient registered twice returns the same row and does
    /// not re-mark dirty after a flush.
    #[test]
    fn register_same_gradient_twice_reuses_row() {
        let mut atlas = GradientCpuAtlas::default();
        let g = distinct_grad(0.5);
        let r1 = atlas.register_stops(&g.stops, g.interp);
        // Flush so subsequent registrations of the same content can
        // be detected as no-ops.
        let _ = atlas.flush();
        let r2 = atlas.register_stops(&g.stops, g.interp);
        assert_eq!(r1, r2);
        assert!(
            atlas.dirty.is_none(),
            "re-registering existing content must not dirty",
        );
    }

    /// Distinct gradients get distinct rows; both leave the atlas
    /// dirty for upload.
    #[test]
    fn register_distinct_gradients_get_distinct_rows() {
        let mut atlas = GradientCpuAtlas::default();
        let _ = atlas.flush();
        let ra = register_for(&mut atlas, distinct_grad(0.1));
        let rb = register_for(&mut atlas, distinct_grad(0.2));
        assert_ne!(ra, rb);
        assert!(atlas.dirty.is_some());
    }

    /// Linear-probe collision handling: if two gradients hash to the
    /// same row id, the second one probes to the next free slot. We
    /// can't easily construct a guaranteed-collision pair without
    /// knowing the FxHash output, so we register many distinct
    /// gradients and verify each gets a unique row (which exercises
    /// the probe path naturally when collisions occur).
    #[test]
    fn register_many_distinct_gradients_all_unique_rows() {
        let mut atlas = GradientCpuAtlas::default();
        let mut seen = std::collections::HashSet::new();
        for i in 0..(ATLAS_ROWS - 1) {
            let g = distinct_grad(i as f32 * 0.01);
            let row = atlas.register_stops(&g.stops, g.interp);
            assert!(
                seen.insert(row),
                "row {} reused across distinct gradients",
                row.0,
            );
            assert_real_row(row);
        }
        assert_eq!(seen.len(), ATLAS_ROWS as usize - 1);
    }

    /// Filling all 255 real slots then registering one more (after a
    /// `flush`, i.e. in the next epoch) evicts the LRU row in
    /// 1..ATLAS_ROWS — never row 0 (magenta fallback). The new gradient
    /// ends up in the evicted slot; the previously resident row's
    /// content hash is gone, while a surviving gradient re-registers
    /// onto its exact original row (hit path).
    #[test]
    fn register_full_atlas_evicts_lru_and_preserves_row_zero() {
        let mut atlas = GradientCpuAtlas::default();
        let mut filled_rows: Vec<LutRow> = Vec::with_capacity((ATLAS_ROWS - 1) as usize);
        for i in 0..(ATLAS_ROWS - 1) {
            filled_rows.push(register_for(&mut atlas, distinct_grad(i as f32 * 0.01)));
        }
        // Re-touch every gradient except index 0 so the very first
        // registration's row is unambiguously the LRU.
        for i in 1..(ATLAS_ROWS - 1) {
            register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
        }
        // Epoch boundary: everything above was registered "this frame"
        // and is eviction-exempt until a flush.
        let _ = atlas.flush();
        let lru = filled_rows[0];
        // Push one more distinct gradient → forces eviction.
        let new_row = register_for(&mut atlas, distinct_grad(9999.0));
        assert_ne!(new_row.0, 0, "row 0 (magenta) must never be evicted");
        assert_eq!(
            new_row, lru,
            "newest registration must land in the LRU slot",
        );
        // A surviving gradient re-registers onto its exact original row.
        let survivor = register_for(&mut atlas, distinct_grad(0.01));
        assert_eq!(
            survivor, filled_rows[1],
            "surviving content must reuse its original row exactly",
        );
        // Row 0 still magenta after eviction.
        let magenta = ColorF16::from(Color::linear_rgba(1.0, 0.0, 1.0, 1.0));
        assert!(atlas.baked[0].iter().all(|&t| t == magenta));
    }

    /// 255 distinct registrations then a 256th in the SAME epoch must
    /// panic: every row's `LutRow` id is already captured in this
    /// frame's draw payloads, so evicting any would silently paint the
    /// wrong gradient — the capacity crash is the correct outcome.
    #[test]
    #[should_panic(expected = "gradient atlas exhausted")]
    fn full_atlas_same_epoch_overflow_panics() {
        let mut atlas = GradientCpuAtlas::default();
        for i in 0..(ATLAS_ROWS - 1) {
            register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
        }
        register_for(&mut atlas, distinct_grad(9999.0));
    }

    /// The hit path stamps the epoch too: re-registering all 255
    /// resident gradients after a flush re-protects every row, so a
    /// 256th distinct gradient in that same epoch must panic rather
    /// than evict a row whose id this frame's draws already hold.
    #[test]
    #[should_panic(expected = "gradient atlas exhausted")]
    fn full_atlas_all_hit_this_epoch_panics() {
        let mut atlas = GradientCpuAtlas::default();
        for i in 0..(ATLAS_ROWS - 1) {
            register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
        }
        let _ = atlas.flush();
        // New epoch: every row re-registered via the hit path.
        for i in 0..(ATLAS_ROWS - 1) {
            register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
        }
        register_for(&mut atlas, distinct_grad(9999.0));
    }

    /// Hit-path bumps the row stamp: a gradient registered first, then
    /// re-registered after others, must survive eviction even when the
    /// table fills.
    #[test]
    fn register_hit_bumps_stamp_protecting_recent_content() {
        let mut atlas = GradientCpuAtlas::default();
        let pinned = distinct_grad(0.0);
        let pinned_row = register_for(&mut atlas, pinned.clone());
        // Fill 253 more rows.
        for i in 1..(ATLAS_ROWS - 2) {
            register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
        }
        // Re-touch the pinned gradient so its stamp is now the largest.
        let r = register_for(&mut atlas, pinned);
        assert_eq!(r, pinned_row, "re-register must reuse the same row");
        // Epoch boundary so the eviction below is legal (nothing above
        // is referenced by the "current frame" anymore).
        let _ = atlas.flush();
        // Two more distinct registrations: the second forces eviction.
        // The pinned row's recent stamp must keep it alive.
        register_for(&mut atlas, distinct_grad(1000.0));
        let evicted_row = register_for(&mut atlas, distinct_grad(1001.0));
        assert_ne!(
            evicted_row, pinned_row,
            "recently touched row must not be evicted",
        );
    }

    /// Evicting a row then re-registering its original content re-bakes
    /// into some slot; the row is restored, no panics, atlas remains
    /// usable. Pin the round-trip explicitly so a future eviction-bug
    /// that loses content silently is caught.
    #[test]
    fn evicted_content_can_be_re_registered() {
        let mut atlas = GradientCpuAtlas::default();
        let first = distinct_grad(0.0);
        let _ = register_for(&mut atlas, first.clone());
        // Fill, cross the epoch boundary, then force eviction of `first`
        // (oldest stamp).
        for i in 1..(ATLAS_ROWS - 1) {
            register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
        }
        let _ = atlas.flush();
        register_for(&mut atlas, distinct_grad(9999.0));
        // Re-register `first` — must succeed and return a valid row.
        let reborn = register_for(&mut atlas, first);
        assert_real_row(reborn);
    }

    /// `flush` returns `Some(...)` once after a register, then `None`
    /// until the next register. Idle-frame upload is zero bytes.
    #[test]
    fn flush_returns_bytes_once_then_none() {
        let mut atlas = GradientCpuAtlas::default();
        register_for(&mut atlas, distinct_grad(0.3));
        assert!(atlas.flush().is_some(), "dirty atlas must yield bytes");
        assert!(
            atlas.flush().is_none(),
            "second flush without register is none"
        );
    }

    /// (stops, interp) keying is variant-agnostic: a linear and a
    /// radial gradient with matching stops + interp share one atlas
    /// row. Geometry differs in the shader (per-fragment `t`), but the
    /// LUT bake doesn't depend on it.
    #[test]
    fn register_stops_dedups_across_variants() {
        let mut atlas = GradientCpuAtlas::default();
        let stops = [
            Stop::new(0.0, ColorU8::rgb(255, 64, 0)),
            Stop::new(1.0, ColorU8::rgb(0, 128, 255)),
        ];
        let r_linear = atlas.register_stops(&stops, Interp::Oklab);
        let r_radial = atlas.register_stops(&stops, Interp::Oklab);
        assert_eq!(r_linear, r_radial);
        // Same stops, different interp → different row.
        let r_other_interp = atlas.register_stops(&stops, Interp::Linear);
        assert_ne!(r_linear, r_other_interp);
    }

    /// Idle atlas (no registrations beyond magenta init) hits the
    /// `Some` branch once for the magenta upload — covering exactly the
    /// one dirty row (row 0, 2048 bytes), not the whole 512 KB atlas —
    /// then stays clean.
    #[test]
    fn freshly_constructed_atlas_flushes_magenta_once() {
        let mut atlas = GradientCpuAtlas::default();
        {
            let first = atlas.flush().expect("first flush carries magenta init");
            assert_eq!(first.first_row, 0);
            assert_eq!(first.bytes.len(), size_of::<LutRowTexels>());
        }
        assert!(atlas.flush().is_none());
    }

    /// The flush range covers exactly the rows touched since the last
    /// flush: one baked row → that single 2048-byte row at its own
    /// index; two scattered rows → the contiguous min..=max span
    /// (`(max - min + 1) × 2048` bytes starting at min); nothing dirty
    /// → `None`.
    #[test]
    fn flush_range_covers_min_to_max_dirty_rows() {
        let mut atlas = GradientCpuAtlas::default();
        let _ = atlas.flush(); // drain the magenta init row
        // Single row: range is exactly [row, row].
        let ra = register_for(&mut atlas, distinct_grad(0.1));
        {
            let f = atlas.flush().expect("one baked row must flush");
            assert_eq!(f.first_row, ra.0);
            assert_eq!(f.bytes.len(), size_of::<LutRowTexels>());
        }
        // Two scattered rows: range spans min..=max, whole rows.
        let rb = register_for(&mut atlas, distinct_grad(0.2));
        let rc = register_for(&mut atlas, distinct_grad(0.3));
        let (min, max) = (rb.0.min(rc.0), rb.0.max(rc.0));
        {
            let f = atlas.flush().expect("two baked rows must flush");
            assert_eq!(f.first_row, min);
            assert_eq!(
                f.bytes.len(),
                (max - min + 1) as usize * size_of::<LutRowTexels>(),
            );
        }
        // Clean atlas: nothing to upload.
        assert!(atlas.flush().is_none());
    }
}
