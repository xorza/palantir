// Step 1 of the brushes slice-2 plan: bake function only. Step 2 adds
// `GradientCpuAtlas` (which calls into here); until then the bake API
// has no in-crate callers outside tests.
#![allow(dead_code)]

//! CPU side of the gradient LUT atlas. This file currently exports
//! only the bake function ([`bake_linear`]); step 2 of the brushes
//! slice-2 plan adds the [`GradientCpuAtlas`] struct on top.
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
use crate::primitives::brush::{Interp, LinearGradient, Stop};
use crate::primitives::color::{Color, Srgb8, linear_to_oklab, oklab_to_linear};
use std::hash::{Hash, Hasher};

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
/// Edge clamp: `t < first_stop.offset` paints `first_stop.color`;
/// `t > last_stop.offset` paints `last_stop.color`. Spread modes
/// (`Pad`/`Repeat`/`Reflect`) are applied **shader-side** on the
/// sampling `t` coordinate, not at bake time — one row serves all
/// spread modes for the same gradient.
pub(crate) fn bake_linear(g: &LinearGradient, out: &mut [u8; LUT_ROW_BYTES]) {
    // Sort stops by offset into a stack scratch. 8 elements max, simple
    // insertion sort beats any allocation. Equal offsets stay in input
    // order (stable) so a hard-transition pair (`(0.5, A), (0.5, B)`)
    // picks `A` on the left, `B` on the right.
    let mut stops: [Stop; crate::primitives::brush::MAX_STOPS] = Default::default();
    let n = g.stops.len();
    stops[..n].copy_from_slice(&g.stops[..]);
    for i in 1..n {
        let mut j = i;
        while j > 0 && stops[j - 1].offset > stops[j].offset {
            stops.swap(j - 1, j);
            j -= 1;
        }
    }

    let first_color = stops[0].color;
    let last_color = stops[n - 1].color;

    for i in 0..LUT_ROW_TEXELS {
        let t = i as f32 / (LUT_ROW_TEXELS - 1) as f32;
        let texel = lerp_at(&stops[..n], first_color, last_color, t, g.interp);
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
    if t <= stops[0].offset {
        return first;
    }
    if t >= stops[stops.len() - 1].offset {
        return last;
    }
    // Find the bracketing pair (a, b) where a.offset <= t <= b.offset.
    // Linear scan — N ≤ 8, dominant cost is the actual lerp math.
    let mut i = 1;
    while i < stops.len() && stops[i].offset < t {
        i += 1;
    }
    let a = stops[i - 1];
    let b = stops[i];
    let denom = b.offset - a.offset;
    // Equal-offset hard transition: pick the right-hand stop (we're
    // past the boundary because the early `t <= stops[0].offset`
    // guard already handled `t == a.offset` for the leftmost stop).
    let u = if denom.abs() <= f32::EPSILON {
        return b.color;
    } else {
        (t - a.offset) / denom
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

/// Sentinel for "no gradient registered in this row." Row indices are
/// `u32` because the GPU consumes them; `u32::MAX` reads as "invalid"
/// at the call site.
pub(crate) const INVALID_ROW: u32 = u32::MAX;

/// CPU side of the gradient LUT atlas. Owns the baked row bytes and a
/// content-hash → row-id map; the backend mirrors this into a wgpu
/// texture each frame by draining `take_dirty()`.
///
/// Row 0 is reserved as a magenta-fill fallback. Slots 1..ATLAS_ROWS
/// are content-hashed. Linear-probed; on probe-failure (atlas full),
/// asserts. Slice-2 baseline: assert; real LRU eviction lives in 2.5.
pub(crate) struct GradientCpuAtlas {
    /// `Some(content_hash)` per row occupied by a gradient; `None` for
    /// free rows. Row 0 holds the magenta-fallback marker (`Some(0)`).
    rows: [Option<u64>; ATLAS_ROWS as usize],
    /// Baked LUT row bytes, indexed by row id. Row 0's contents are
    /// the magenta-fallback fill. Storage is a single 256 KB heap
    /// allocation — `Vec<[u8; 1024]>` is contiguous, so casting to
    /// `&[u8]` for the GPU upload is a free reinterpret.
    baked: Vec<[u8; LUT_ROW_BYTES]>,
    /// Any row changed since the last `flush`. Per-row tracking is
    /// overkill at 256 KB total: one `queue.write_texture` call of
    /// 256 KB beats N calls of 1 KB each on the common warmup path
    /// (N ≥ 2 distinct gradients) thanks to fixed-cost API overhead
    /// per call; for the N = 1 edge case the two are roughly tied.
    dirty: bool,
}

impl Default for GradientCpuAtlas {
    fn default() -> Self {
        let mut atlas = Self {
            rows: [None; ATLAS_ROWS as usize],
            baked: vec![[0u8; LUT_ROW_BYTES]; ATLAS_ROWS as usize],
            dirty: false,
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
        // Reserve the slot with a sentinel hash so it never gets
        // re-claimed by a real gradient that happens to hash to 0.
        self.rows[0] = Some(0);
        // First-frame upload paints the magenta fallback.
        self.dirty = true;
    }

    /// Find-or-bake the row for `g`. Returns the row id (in
    /// `1..ATLAS_ROWS`). The same gradient registered repeatedly across
    /// frames returns the same row without re-baking; a fresh gradient
    /// gets the next slot via content-hash + linear probing.
    pub(crate) fn register(&mut self, g: &LinearGradient) -> u32 {
        let content_hash = hash_gradient(g);
        // Probe starting at `1 + (hash mod 255)` so row 0 is never
        // claimed by a real gradient.
        let start = 1 + (content_hash % (ATLAS_ROWS as u64 - 1)) as u32;
        for offset in 0..(ATLAS_ROWS - 1) {
            let row = 1 + ((start - 1 + offset) % (ATLAS_ROWS - 1));
            match self.rows[row as usize] {
                Some(h) if h == content_hash => return row,
                None => {
                    // Free slot — bake into it.
                    bake_linear(g, &mut self.baked[row as usize]);
                    self.rows[row as usize] = Some(content_hash);
                    self.dirty = true;
                    return row;
                }
                _ => continue,
            }
        }
        // Slice-2 baseline: atlas full → assert. LRU eviction is a
        // slice-2.5 follow-up (see brushes-slice-2-plan.md open
        // question #4).
        panic!(
            "GradientCpuAtlas full ({} live gradients); LRU eviction not yet implemented",
            ATLAS_ROWS - 1,
        );
    }

    /// If any row changed since the last flush, return the full atlas
    /// bytes (`ATLAS_ROWS × LUT_ROW_BYTES` = 256 KB, contiguous) for
    /// one-shot upload, and clear the dirty flag. Returns `None` when
    /// nothing has changed — the steady-state idle frame uploads
    /// zero bytes.
    pub(crate) fn flush(&mut self) -> Option<&[u8]> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        Some(bytemuck::cast_slice(&self.baked))
    }

    /// Test-only: borrow a baked row's bytes. Production code uses
    /// [`Self::flush`]; this exists so tests can spot-check
    /// magenta-fallback initialisation and bake output for specific
    /// rows.
    #[cfg(test)]
    fn row_bytes(&self, row: u8) -> &[u8; LUT_ROW_BYTES] {
        &self.baked[row as usize]
    }

    /// Test-only: peek the dirty flag without clearing.
    #[cfg(test)]
    fn is_dirty(&self) -> bool {
        self.dirty
    }
}

/// Content hash of a `LinearGradient`. Stable across frames given
/// identical content. Drives row-id derivation in `register`; identical
/// content always picks the same probe start, so the same gradient
/// reuses the same row.
#[inline]
fn hash_gradient(g: &LinearGradient) -> u64 {
    let mut h = FxHasher::new();
    g.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        bake_linear(&g, &mut out);
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
        bake_linear(&g, &mut out);
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
        bake_linear(&g, &mut out);
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
            bake_linear(&g, &mut out);
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
        bake_linear(&g, &mut out);
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
        bake_linear(&g, &mut out);
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
        bake_linear(&g, &mut out);
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
        bake_linear(&g, &mut out);
        // Texel 0 (t=0): clamped to first stop colour.
        assert_eq!(texel(&out, 0).g, 255);
        // Texel 255 (t=1): clamped to last stop colour.
        assert_eq!(texel(&out, LUT_ROW_TEXELS - 1).b, 255);
    }

    // ----- GradientCpuAtlas tests ------------------------------------

    fn distinct_grad(seed: f32) -> LinearGradient {
        LinearGradient::two_stop(seed, Srgb8::rgb(0xff, 0, 0), Srgb8::rgb(0, 0xff, 0))
    }

    /// Row 0 is reserved magenta. Created at construction; dirty list
    /// flags it so the first frame's GPU upload paints the fallback row.
    /// First real registration goes to row 1 (or wherever its hash lands
    /// in 1..ATLAS_ROWS).
    #[test]
    fn row_zero_reserved_as_magenta_fallback() {
        let atlas = GradientCpuAtlas::default();
        // Row 0 is magenta sRGB across all texels.
        let row0 = atlas.row_bytes(0);
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
        let row = atlas.register(&g);
        assert!(
            (1..ATLAS_ROWS).contains(&row),
            "row {row} must be in 1..ATLAS_ROWS"
        );
        assert!(atlas.is_dirty(), "register must mark atlas dirty");
    }

    /// Same gradient registered twice returns the same row and does
    /// not re-mark dirty after a flush.
    #[test]
    fn register_same_gradient_twice_reuses_row() {
        let mut atlas = GradientCpuAtlas::default();
        let g = distinct_grad(0.5);
        let r1 = atlas.register(&g);
        // Flush so subsequent registrations of the same content can
        // be detected as no-ops.
        let _ = atlas.flush();
        let r2 = atlas.register(&g);
        assert_eq!(r1, r2);
        assert!(
            !atlas.is_dirty(),
            "re-registering existing content must not dirty",
        );
    }

    /// Distinct gradients get distinct rows; both leave the atlas
    /// dirty for upload.
    #[test]
    fn register_distinct_gradients_get_distinct_rows() {
        let mut atlas = GradientCpuAtlas::default();
        let _ = atlas.flush();
        let ra = atlas.register(&distinct_grad(0.1));
        let rb = atlas.register(&distinct_grad(0.2));
        assert_ne!(ra, rb);
        assert!(atlas.is_dirty());
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
            let row = atlas.register(&g);
            assert!(
                seen.insert(row),
                "row {row} reused across distinct gradients"
            );
            assert!((1..ATLAS_ROWS).contains(&row));
        }
        assert_eq!(seen.len(), ATLAS_ROWS as usize - 1);
    }

    /// Filling all 255 real slots, then asking for one more, panics
    /// with the slice-2 "atlas full" message. Real LRU eviction is a
    /// slice-2.5 follow-up.
    #[test]
    #[should_panic(expected = "GradientCpuAtlas full")]
    fn register_panics_when_atlas_full() {
        let mut atlas = GradientCpuAtlas::default();
        for i in 0..(ATLAS_ROWS - 1) {
            atlas.register(&distinct_grad(i as f32 * 0.01));
        }
        // 256th distinct gradient (255 real slots already occupied).
        atlas.register(&distinct_grad(9999.0));
    }

    /// `flush` returns `Some(...)` once after a register, then `None`
    /// until the next register. Idle-frame upload is zero bytes.
    #[test]
    fn flush_returns_bytes_once_then_none() {
        let mut atlas = GradientCpuAtlas::default();
        atlas.register(&distinct_grad(0.3));
        assert!(atlas.flush().is_some(), "dirty atlas must yield bytes");
        assert!(
            atlas.flush().is_none(),
            "second flush without register is none"
        );
    }

    /// Idle atlas (no registrations beyond magenta init) hits the
    /// `Some` branch once for the magenta upload, then stays clean.
    #[test]
    fn freshly_constructed_atlas_flushes_magenta_once() {
        let mut atlas = GradientCpuAtlas::default();
        let first = atlas.flush().expect("first flush carries magenta init");
        assert_eq!(first.len(), ATLAS_ROWS as usize * LUT_ROW_BYTES);
        assert!(atlas.flush().is_none());
    }
}
