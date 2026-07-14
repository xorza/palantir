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
