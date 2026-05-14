//! CPU side of the gradient LUT atlas. Bakes stop sequences into LUT
//! rows shared across linear / radial / conic gradient variants; the
//! shader does the per-fragment `t` derivation. See [`bake_stops`] and
//! [`GradientCpuAtlas::register_stops`].
//!
//! ## Bake output convention
//!
//! Each baked row is 256 RGBA texels = 1024 bytes, **straight (non-
//! premultiplied) sRGB**. The backend uploads these into an
//! `Rgba8UnormSrgb` texture; the shader samples and gets linear-RGB
//! `vec4<f32>` for free via the GPU's sRGB decoder. The premul step
//! happens in the shader on the sampled value — same convention as
//! the rest of the pipeline (see "Colour pipeline" in `CLAUDE.md`).
//!
//! ## Interpolation spaces
//!
//! - [`Interp::Srgb`]: lerp `Srgb8` channels directly. Cheapest;
//!   matches old design-tool behaviour (Photoshop pre-2023, Figma).
//! - [`Interp::Linear`]: convert stops `Srgb8 → linear RGB → lerp →
//!   sRGB8`. Physically correct linear blend; shows visible midpoint
//!   dip on saturated complementary pairs (red↔green muddy brown).
//! - [`Interp::Oklab`]: convert `Srgb8 → linear → Oklab → lerp →
//!   linear → sRGB8`. Perceptually uniform; CSS Color 4 default.
//!   Avoids the muddy midpoint without needing a tweaked palette.

use crate::common::hash::Hasher as FxHasher;
use crate::primitives::brush::{Interp, MAX_STOPS, Stop};
use crate::primitives::color::{Color, Srgb8, linear_to_oklab, oklab_to_linear};
use std::cell::Cell;
use std::hash::Hasher;

/// Index into the gradient LUT atlas texture. `LutRow(0)` is the
/// magenta debug fallback (so a stray default value paints obviously
/// wrong); real registrations occupy `1..ATLAS_ROWS`. Newtype keeps
/// the atlas-row identifier from being silently swapped with another
/// `u32` field on `Quad`.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LutRow(pub(crate) u32);

impl LutRow {
    /// Sentinel for solid (non-gradient) quads. The shader only samples
    /// the LUT when `fill_kind` is a gradient, so the value is unused
    /// in that path; a stray `FALLBACK` reaching the sampler paints
    /// magenta.
    pub(crate) const FALLBACK: LutRow = LutRow(0);
}

/// Number of rows in the LUT atlas texture. One row per distinct
/// gradient currently in use. Row 0 is reserved as a debug-magenta
/// fallback (so a `fill_lut_row = 0` from a bug paints obviously
/// wrong); real registrations occupy rows 1..ATLAS_ROWS.
pub(crate) const ATLAS_ROWS: u32 = 256;

/// Width of one baked row in texels. Picked to match the LUT texture's
/// 256-texel width; 256 gives 1 LSB per stride on the parametric axis,
/// well below 8-bit display precision.
pub(crate) const LUT_ROW_TEXELS: usize = 256;

/// Width of one baked row in bytes. Each texel is `[r, g, b, a]: u8`.
pub(crate) const LUT_ROW_BYTES: usize = LUT_ROW_TEXELS * 4;

/// Bake a [`LinearGradient`] into a single LUT row.
///
/// Output is straight (non-premultiplied) sRGB bytes — see module docs.
/// `out` is a `&mut [u8; LUT_ROW_BYTES]`, written in place; the buffer
/// is fully overwritten, no read-before-write.
///
/// Edge clamp: `t < first_stop.offset()` paints `first_stop.color`;
/// `t > last_stop.offset()` paints `last_stop.color`. Spread modes
/// (`Pad`/`Repeat`/`Reflect`) are applied **shader-side** on the
/// sampling `t` coordinate, not at bake time — one row serves all
/// spread modes for the same gradient.
pub(crate) fn bake_stops(stops: &[Stop], interp: Interp, out: &mut [u8; LUT_ROW_BYTES]) {
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

    let first_color = sorted[0].color;
    let last_color = sorted[n - 1].color;

    for i in 0..LUT_ROW_TEXELS {
        let t = i as f32 / (LUT_ROW_TEXELS - 1) as f32;
        let texel = lerp_at(&sorted[..n], first_color, last_color, t, interp);
        let off = i * 4;
        out[off] = texel.r;
        out[off + 1] = texel.g;
        out[off + 2] = texel.b;
        out[off + 3] = texel.a;
    }
}

/// Resolve the colour at parametric `t ∈ 0..=1`. Edge clamp outside the
/// first/last stop offsets; bracket-and-lerp in between.
#[inline]
fn lerp_at(stops: &[Stop], first: Srgb8, last: Srgb8, t: f32, interp: Interp) -> Srgb8 {
    // Edge clamp: outside the parametric span, return the edge stop's
    // colour. Spread mode handles "outside the geometry"; this only
    // handles "outside the stop offsets", i.e. when the leftmost stop
    // is at 0.2 and the rightmost at 0.8.
    if t <= stops[0].offset() {
        return first;
    }
    if t >= stops[stops.len() - 1].offset() {
        return last;
    }
    // Find the bracketing pair (a, b) where a.offset() <= t <= b.offset().
    // Linear scan — N ≤ 8, dominant cost is the actual lerp math.
    let mut i = 1;
    while i < stops.len() && stops[i].offset() < t {
        i += 1;
    }
    let a = stops[i - 1];
    let b = stops[i];
    let denom = b.offset() - a.offset();
    // Equal-offset hard transition: pick the right-hand stop (we're
    // past the boundary because the early `t <= stops[0].offset()`
    // guard already handled `t == a.offset()` for the leftmost stop).
    let u = if denom.abs() <= f32::EPSILON {
        return b.color;
    } else {
        (t - a.offset()) / denom
    };

    match interp {
        Interp::Srgb => lerp_srgb8(a.color, b.color, u),
        Interp::Linear => lerp_linear(a.color, b.color, u),
        Interp::Oklab => lerp_oklab(a.color, b.color, u),
    }
}

/// Lerp two `Srgb8` colours by treating each channel as `u8 / 255`,
/// lerping in that 0..1 sRGB space, then quantising back. No linear /
/// Oklab roundtrip — cheapest mode; results match the old "lerp the
/// hex bytes" convention.
#[inline]
fn lerp_srgb8(a: Srgb8, b: Srgb8, u: f32) -> Srgb8 {
    Srgb8 {
        r: lerp_u8(a.r, b.r, u),
        g: lerp_u8(a.g, b.g, u),
        b: lerp_u8(a.b, b.b, u),
        a: lerp_u8(a.a, b.a, u),
    }
}

#[inline]
fn lerp_u8(a: u8, b: u8, u: f32) -> u8 {
    let a = a as f32;
    let b = b as f32;
    (a + (b - a) * u).round().clamp(0.0, 255.0) as u8
}

/// Lerp in linear-RGB. Stops expand to `Color` (linear f32 via the
/// cubic sRGB→linear curve), lerp componentwise, quantize back to
/// sRGB8.
#[inline]
fn lerp_linear(a: Srgb8, b: Srgb8, u: f32) -> Srgb8 {
    let ca: Color = a.into();
    let cb: Color = b.into();
    Color {
        r: ca.r + (cb.r - ca.r) * u,
        g: ca.g + (cb.g - ca.g) * u,
        b: ca.b + (cb.b - ca.b) * u,
        a: ca.a + (cb.a - ca.a) * u,
    }
    .to_srgb8()
}

/// Lerp in Oklab. Stops expand to linear-RGB → Oklab, lerp the L/a/b
/// triplet, back through Oklab → linear → sRGB8. Alpha lerps in the
/// stored 0..1 linear-alpha space (alpha doesn't participate in the
/// L/a/b transform).
#[inline]
fn lerp_oklab(a: Srgb8, b: Srgb8, u: f32) -> Srgb8 {
    let ca: Color = a.into();
    let cb: Color = b.into();
    let lab_a = linear_to_oklab(ca.r, ca.g, ca.b);
    let lab_b = linear_to_oklab(cb.r, cb.g, cb.b);
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
        a: ca.a + (cb.a - ca.a) * u,
    }
    .to_srgb8()
}

/// CPU side of the gradient LUT atlas. Owns the baked row bytes and a
/// content-hash → row-id map; the backend mirrors this into a wgpu
/// texture each frame by draining `take_dirty()`.
///
/// Row 0 is reserved as a magenta-fill fallback and never evicted.
/// Slots 1..ATLAS_ROWS are content-hashed and linear-probed. When the
/// table is full and the requested content isn't already resident, the
/// LRU row (smallest `last_used`) is evicted and re-baked in place.
pub(crate) struct GradientCpuAtlas {
    /// `Some(content_hash)` per row occupied by a gradient; `None` for
    /// free rows. Row 0 is unreachable from the probe (which scans
    /// `1..ATLAS_ROWS`) so its slot stays `None` — the magenta-fill
    /// payload in `baked[0]` is the real fallback contract.
    rows: [Option<u64>; ATLAS_ROWS as usize],
    /// Baked LUT row bytes, indexed by row id. Row 0's contents are
    /// the magenta-fallback fill. Storage is a single 256 KB heap
    /// allocation — `Vec<[u8; 1024]>` is contiguous, so casting to
    /// `&[u8]` for the GPU upload is a free reinterpret.
    baked: Vec<[u8; LUT_ROW_BYTES]>,
    /// Per-row "last touched" timestamp. Bumped on every `register_stops`
    /// hit and on bake. The LRU victim is the row with the smallest
    /// stamp; row 0 is excluded. `u64` so wrap is unreachable in any
    /// realistic workload (a `u32` at 60 fps × 200 registers/frame
    /// rolls over in ~10 years and silently mis-evicts on wrap).
    last_used: [u64; ATLAS_ROWS as usize],
    /// Monotonic register counter. Each `register_stops` call bumps
    /// it and stamps the touched row, so within a single frame later
    /// registers are "newer" than earlier ones (fine — eviction needs
    /// a strict-order comparator, not wall-clock semantics).
    clock: u64,
    /// Any row changed since the last `flush`. Per-row tracking is
    /// overkill at 256 KB total: one `queue.write_texture` call of
    /// 256 KB beats N calls of 1 KB each on the common warmup path
    /// (N ≥ 2 distinct gradients) thanks to fixed-cost API overhead
    /// per call; for the N = 1 edge case the two are roughly tied.
    ///
    /// `Cell` because `flush` clears it on upload but is called
    /// through `&self` — lets the backend take `&RenderBuffer`
    /// instead of `&mut RenderBuffer` for the entire submit path.
    dirty: Cell<bool>,
}

impl Default for GradientCpuAtlas {
    fn default() -> Self {
        let mut atlas = Self {
            rows: [None; ATLAS_ROWS as usize],
            baked: vec![[0u8; LUT_ROW_BYTES]; ATLAS_ROWS as usize],
            last_used: [0; ATLAS_ROWS as usize],
            clock: 0,
            dirty: Cell::new(false),
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
        let row = &mut self.baked[0];
        for i in 0..LUT_ROW_TEXELS {
            let off = i * 4;
            row[off] = 0xff;
            row[off + 1] = 0x00;
            row[off + 2] = 0xff;
            row[off + 3] = 0xff;
        }
        // No `rows[0]` sentinel: the probe range is `1..ATLAS_ROWS`,
        // so row 0 is unreachable regardless of what hash a real
        // gradient produces.
        //
        // First-frame upload paints the magenta fallback.
        self.dirty.set(true);
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
        let stamp = self.clock;
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
                    self.last_used[row as usize] = stamp;
                    return LutRow(row);
                }
                None => {
                    bake_stops(stops, interp, &mut self.baked[row as usize]);
                    self.rows[row as usize] = Some(content_hash);
                    self.last_used[row as usize] = stamp;
                    self.dirty.set(true);
                    return LutRow(row);
                }
                _ => continue,
            }
        }
        // Atlas full: evict the row with the smallest stamp. Row 0
        // (magenta fallback) is permanent — start the scan at 1.
        let victim = self.lru_victim();
        bake_stops(stops, interp, &mut self.baked[victim as usize]);
        self.rows[victim as usize] = Some(content_hash);
        self.last_used[victim as usize] = stamp;
        self.dirty.set(true);
        LutRow(victim)
    }

    /// Scan rows 1..ATLAS_ROWS for the smallest `last_used` stamp.
    /// Always returns a row id ≥ 1 (row 0 excluded) — the magenta
    /// fallback is permanent.
    fn lru_victim(&self) -> u32 {
        let mut best_row: u32 = 1;
        let mut best_stamp = self.last_used[1];
        for row in 2..ATLAS_ROWS {
            let s = self.last_used[row as usize];
            if s < best_stamp {
                best_stamp = s;
                best_row = row;
            }
        }
        best_row
    }

    /// If any row changed since the last flush, return the full atlas
    /// bytes (`ATLAS_ROWS × LUT_ROW_BYTES` = 256 KB, contiguous) for
    /// one-shot upload, and clear the dirty flag. Returns `None` when
    /// nothing has changed — the steady-state idle frame uploads
    /// zero bytes.
    pub(crate) fn flush(&self) -> Option<&[u8]> {
        if !self.dirty.replace(false) {
            return None;
        }
        Some(bytemuck::cast_slice(&self.baked))
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
    use super::*;
    use crate::primitives::brush::LinearGradient;

    fn texel(out: &[u8; LUT_ROW_BYTES], i: usize) -> Srgb8 {
        Srgb8 {
            r: out[i * 4],
            g: out[i * 4 + 1],
            b: out[i * 4 + 2],
            a: out[i * 4 + 3],
        }
    }

    /// `Interp::Srgb`: midpoint of a 2-stop linear gradient from
    /// `#000000` to `#ffffff` should be byte-equal `(0x80, 0x80, 0x80)`
    /// (well, `127` or `128` after `.round()` — sRGB-space halfway).
    /// Pinned because it's the simplest "did the basic plumbing work"
    /// signal.
    #[test]
    fn srgb_midpoint_black_to_white_is_128() {
        let g = LinearGradient::two_stop(0.0, Srgb8::BLACK, Srgb8::WHITE).with_interp(Interp::Srgb);
        let mut out = [0u8; LUT_ROW_BYTES];
        bake_stops(&g.stops, g.interp, &mut out);
        // Texel index 127.5 doesn't exist; check both bracket texels.
        // 127/255 ≈ 0.498 → ~127; 128/255 ≈ 0.502 → ~128.
        let mid = texel(&out, 127);
        assert!(
            (mid.r as i16 - 127).abs() <= 1,
            "midpoint r={} not ~127",
            mid.r,
        );
        assert_eq!(mid.r, mid.g);
        assert_eq!(mid.g, mid.b);
        assert_eq!(mid.a, 255);
    }

    /// `Interp::Linear`: midpoint of black→white in linear-RGB space
    /// returns a brighter grey than sRGB-space lerp because linear 0.5
    /// re-encodes to ~0.735 sRGB (≈ `#bcbcbc`). Pin the rough range
    /// (`>= 180`) to catch a regression that accidentally falls back
    /// to sRGB lerp.
    #[test]
    fn linear_midpoint_black_to_white_is_brighter_than_srgb_midpoint() {
        let g =
            LinearGradient::two_stop(0.0, Srgb8::BLACK, Srgb8::WHITE).with_interp(Interp::Linear);
        let mut out = [0u8; LUT_ROW_BYTES];
        bake_stops(&g.stops, g.interp, &mut out);
        let mid = texel(&out, 127);
        assert!(
            mid.r >= 180,
            "linear-RGB midpoint should be visibly brighter than sRGB 128, got {}",
            mid.r,
        );
        assert_eq!(mid.r, mid.g);
        assert_eq!(mid.g, mid.b);
        assert_eq!(mid.a, 255);
    }

    /// `Interp::Oklab`: red→green midpoint should *not* be muddy
    /// brown (which is what linear-RGB lerps produce). Specifically,
    /// the green channel at midpoint should be high (Oklab keeps
    /// luminance up through the midpoint by traversing yellow-ish
    /// hues rather than dipping through dark brown).
    #[test]
    fn oklab_red_to_green_midpoint_avoids_muddy_brown() {
        let red = Srgb8::rgb(255, 0, 0);
        let green = Srgb8::rgb(0, 255, 0);
        let g = LinearGradient::two_stop(0.0, red, green).with_interp(Interp::Oklab);
        let mut out = [0u8; LUT_ROW_BYTES];
        bake_stops(&g.stops, g.interp, &mut out);
        let mid = texel(&out, 127);
        // Both channels should be non-trivial at midpoint — Oklab
        // hits a yellowish midpoint, not the dark muddy brown that
        // linear lerp produces (where r, g both end up ~125).
        assert!(
            mid.r > 200 && mid.g > 150,
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
        let c0 = Srgb8::rgb(11, 22, 33);
        let c1 = Srgb8::rgb(244, 233, 222);
        for interp in [Interp::Srgb, Interp::Linear, Interp::Oklab] {
            let g = LinearGradient::two_stop(0.0, c0, c1).with_interp(interp);
            let mut out = [0u8; LUT_ROW_BYTES];
            bake_stops(&g.stops, g.interp, &mut out);
            let first = texel(&out, 0);
            let last = texel(&out, LUT_ROW_TEXELS - 1);
            // sRGB matches exactly; linear/Oklab roundtrip through f32
            // can drift ±1 LSB on extreme bytes — accept that.
            let drift = if matches!(interp, Interp::Srgb) { 0 } else { 1 };
            for (chan, (got, want)) in [
                (first.r, c0.r),
                (first.g, c0.g),
                (first.b, c0.b),
                (last.r, c1.r),
                (last.g, c1.g),
                (last.b, c1.b),
            ]
            .into_iter()
            .enumerate()
            {
                assert!(
                    (got as i16 - want as i16).abs() <= drift,
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
            Srgb8::rgb(0, 0, 0),   // stop at 0.0
            Srgb8::rgb(255, 0, 0), // stop at 0.5
            Srgb8::rgb(0, 0, 255), // stop at 1.0
        )
        .with_interp(Interp::Srgb);
        let mut out = [0u8; LUT_ROW_BYTES];
        bake_stops(&g.stops, g.interp, &mut out);
        // Texel at i=64 ≈ t=0.251 → halfway between stops 0 and 1.
        // r channel: lerp(0, 255, 0.502) ≈ 128.
        let q = texel(&out, 64);
        assert!(
            (q.r as i16 - 128).abs() <= 2,
            "quarter-texel r={} not ~128 (bracketing first pair)",
            q.r,
        );
        // b should still be near 0 (stop 2's b=255 isn't reached yet).
        assert!(q.b <= 5, "quarter-texel b={} leaked from stop 2", q.b);
    }

    /// Pin the byte layout: 256 texels × 4 bytes = 1024 bytes total,
    /// in `[r, g, b, a]` order per texel.
    #[test]
    fn lut_row_byte_layout() {
        assert_eq!(LUT_ROW_BYTES, 1024);
        assert_eq!(LUT_ROW_TEXELS, 256);
        let g = LinearGradient::two_stop(0.0, Srgb8::rgb(1, 2, 3), Srgb8::rgb(4, 5, 6));
        let mut out = [0u8; LUT_ROW_BYTES];
        bake_stops(&g.stops, g.interp, &mut out);
        // First texel: explicit byte order check.
        assert_eq!(&out[..4], &[1, 2, 3, 255]);
        // Last texel.
        assert_eq!(&out[1020..1024], &[4, 5, 6, 255]);
    }

    /// Unsorted stops are sorted at bake time. Authors shouldn't rely
    /// on this — `LinearGradient::new` accepts any order — but the
    /// bake must produce a sensible output regardless.
    #[test]
    fn unsorted_stops_get_sorted_at_bake() {
        let stops = [
            Stop::new(1.0, Srgb8::rgb(255, 0, 0)), // out of order
            Stop::new(0.0, Srgb8::rgb(0, 0, 255)),
        ];
        let g = LinearGradient::new(0.0, stops);
        let mut out = [0u8; LUT_ROW_BYTES];
        bake_stops(&g.stops, g.interp, &mut out);
        // First texel should be blue (the stop at 0.0), last should be red.
        let first = texel(&out, 0);
        let last = texel(&out, LUT_ROW_TEXELS - 1);
        assert_eq!((first.r, first.g, first.b), (0, 0, 255));
        assert_eq!((last.r, last.g, last.b), (255, 0, 0));
    }

    /// Stops covering only `0.25..0.75` clamp at the edges: texels
    /// before 0.25 paint the first stop's colour, after 0.75 paint
    /// the last stop's colour. Spread modes (Pad/Repeat/Reflect) are
    /// applied later in the shader on `t`, not here; the bake just
    /// emits the parametric range with edge-clamp behaviour.
    #[test]
    fn partial_range_clamps_at_edges() {
        let stops = [
            Stop::new(0.25, Srgb8::rgb(0, 255, 0)),
            Stop::new(0.75, Srgb8::rgb(0, 0, 255)),
        ];
        let g = LinearGradient::new(0.0, stops);
        let mut out = [0u8; LUT_ROW_BYTES];
        bake_stops(&g.stops, g.interp, &mut out);
        // Texel 0 (t=0): clamped to first stop colour.
        assert_eq!(texel(&out, 0).g, 255);
        // Texel 255 (t=1): clamped to last stop colour.
        assert_eq!(texel(&out, LUT_ROW_TEXELS - 1).b, 255);
    }

    // ----- GradientCpuAtlas tests ------------------------------------

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
        LinearGradient::two_stop(0.0, Srgb8::rgb(r, g, b), Srgb8::rgb(0, 0xff, 0))
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
        // Row 0 is magenta sRGB across all texels.
        let row0 = &atlas.baked[0];
        for i in 0..LUT_ROW_TEXELS {
            let off = i * 4;
            assert_eq!(&row0[off..off + 4], &[0xff, 0x00, 0xff, 0xff]);
        }
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
        assert!(atlas.dirty.get(), "register must mark atlas dirty");
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
            !atlas.dirty.get(),
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
        assert!(atlas.dirty.get());
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

    /// Filling all 255 real slots then registering one more evicts
    /// the LRU row in 1..ATLAS_ROWS — never row 0 (magenta fallback).
    /// The new gradient ends up in the evicted slot; the previously
    /// resident row's content hash is gone.
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
        let lru = filled_rows[0];
        // Push one more distinct gradient → forces eviction.
        let new_row = register_for(&mut atlas, distinct_grad(9999.0));
        assert_ne!(new_row.0, 0, "row 0 (magenta) must never be evicted");
        assert_eq!(
            new_row, lru,
            "newest registration must land in the LRU slot",
        );
        // Row 0 still magenta after eviction.
        for i in 0..LUT_ROW_TEXELS {
            let off = i * 4;
            assert_eq!(&atlas.baked[0][off..off + 4], &[0xff, 0x00, 0xff, 0xff]);
        }
    }

    /// Hit-path bumps the row stamp: a gradient registered first, then
    /// re-registered after others, must survive eviction even when the
    /// table fills.
    #[test]
    fn register_hit_bumps_stamp_protecting_recent_content() {
        let mut atlas = GradientCpuAtlas::default();
        let pinned = distinct_grad(0.0);
        let pinned_row = register_for(&mut atlas, pinned);
        // Fill 253 more rows.
        for i in 1..(ATLAS_ROWS - 2) {
            register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
        }
        // Re-touch the pinned gradient so its stamp is now the largest.
        let r = register_for(&mut atlas, pinned);
        assert_eq!(r, pinned_row, "re-register must reuse the same row");
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
        let _ = register_for(&mut atlas, first);
        // Fill + force eviction of `first` (oldest stamp).
        for i in 1..(ATLAS_ROWS - 1) {
            register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
        }
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
            Stop::new(0.0, Srgb8::rgb(255, 64, 0)),
            Stop::new(1.0, Srgb8::rgb(0, 128, 255)),
        ];
        let r_linear = atlas.register_stops(&stops, Interp::Oklab);
        let r_radial = atlas.register_stops(&stops, Interp::Oklab);
        assert_eq!(r_linear, r_radial);
        // Same stops, different interp → different row.
        let r_other_interp = atlas.register_stops(&stops, Interp::Linear);
        assert_ne!(r_linear, r_other_interp);
    }

    /// Idle atlas (no registrations beyond magenta init) hits the
    /// `Some` branch once for the magenta upload, then stays clean.
    #[test]
    fn freshly_constructed_atlas_flushes_magenta_once() {
        let atlas = GradientCpuAtlas::default();
        let first = atlas.flush().expect("first flush carries magenta init");
        assert_eq!(first.len(), ATLAS_ROWS as usize * LUT_ROW_BYTES);
        assert!(atlas.flush().is_none());
    }
}
