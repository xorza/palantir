use crate::common::hash::Hasher;
use crate::primitives::brush::gradient::Interp;
use crate::primitives::brush::gradient::linear::LinearGradient;
use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::color::ColorU8;
use crate::renderer::gradient_atlas::*;
use crate::renderer::texture_limit::TextureLimit;
use std::collections::HashSet;
use std::hash::Hasher as _;

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
    let g =
        LinearGradient::two_stop(0.0, ColorU8::BLACK, ColorU8::WHITE).with_interp(Interp::Linear);
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
    let g = LinearGradient::builder(0.0)
        .stop(0.0, ColorU8::rgb(0, 0, 0))
        .stop(0.5, ColorU8::rgb(255, 0, 0))
        .stop(1.0, ColorU8::rgb(0, 0, 255))
        .with_interp(Interp::Linear)
        .build();
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

/// The segment search resumes where the previous texel left it, which is
/// sound only because `t` never decreases across the row. A scan that
/// re-brackets from the first segment every time is the reference: eight
/// stops, a hard stop (two at one offset) and segments narrower than the
/// texel step, every texel bit-identical.
#[test]
fn cursor_scan_matches_restart_scan_across_eight_stops() {
    /// The pre-cursor bracketing, transcribed: restart at segment 1 and
    /// walk forward. Same arithmetic in the same order, so agreement is
    /// exact rather than approximate.
    fn restart_scan(stops: &GradientStops, t: f32) -> Color {
        let linear: Vec<Color> = stops.iter().map(|stop| stop.color.into()).collect();
        if t <= stops[0].offset() {
            return linear[0];
        }
        if t >= stops[stops.len() - 1].offset() {
            return linear[stops.len() - 1];
        }
        let mut upper = 1;
        while upper < stops.len() && stops[upper].offset() < t {
            upper += 1;
        }
        let lower_offset = stops[upper - 1].offset();
        let upper_offset = stops[upper].offset();
        let denominator = upper_offset - lower_offset;
        if denominator.abs() <= f32::EPSILON {
            return linear[upper];
        }
        Color::lerp(
            linear[upper - 1],
            linear[upper],
            (t - lower_offset) / denominator,
        )
    }

    let g = LinearGradient::new(
        0.0,
        [
            Stop::new(0.0, ColorU8::rgb(0, 0, 0)),
            Stop::new(0.002, ColorU8::rgb(255, 0, 0)), // narrower than one texel
            Stop::new(0.25, ColorU8::rgb(0, 255, 0)),
            Stop::new(0.5, ColorU8::rgb(0, 0, 255)),
            Stop::new(0.5, ColorU8::rgb(255, 255, 0)), // hard stop
            Stop::new(0.75, ColorU8::rgb(0, 255, 255)),
            Stop::new(0.9, ColorU8::rgb(255, 0, 255)),
            Stop::new(1.0, ColorU8::rgb(255, 255, 255)),
        ],
    )
    .with_interp(Interp::Linear);
    let mut out = fresh_row();
    bake_stops(&g.stops, g.interp, &mut out);
    for (i, got) in out.iter().enumerate() {
        let t = i as f32 / (LUT_ROW_TEXELS - 1) as f32;
        let want = ColorF16::from(restart_scan(&g.stops, t));
        assert_eq!(*got, want, "texel {i} at t={t}");
    }
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
    let mut h = Hasher::new();
    h.write_u32(seed.to_bits());
    let v = h.finish();
    let r = v as u8;
    let g = (v >> 8) as u8;
    let b = (v >> 16) as u8;
    LinearGradient::two_stop(0.0, ColorU8::rgb(r, g, b), ColorU8::rgb(0, 0xff, 0))
}

fn register_for(atlas: &mut CpuGradientAtlas, g: LinearGradient) -> LutRow {
    atlas.register_stops(&g.stops, g.interp)
}

fn assert_real_row(atlas: &CpuGradientAtlas, row: LutRow) {
    assert!(
        (1..atlas.capacity()).contains(&row.0),
        "row {} must be in 1..{}",
        row.0,
        atlas.capacity(),
    );
}

/// Row 0 is reserved magenta. Created at construction; dirty list
/// flags it so the first frame's GPU upload paints the fallback row.
/// First real registration goes to row 1 (or wherever its hash lands
/// in 1..INITIAL_ATLAS_ROWS).
#[test]
fn row_zero_reserved_as_magenta_fallback() {
    let atlas = CpuGradientAtlas::default();
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
    let mut atlas = CpuGradientAtlas::default();
    let g = distinct_grad(0.1);
    let row = atlas.register_stops(&g.stops, g.interp);
    assert_real_row(&atlas, row);
    assert!(atlas.dirty.is_some(), "register must mark atlas dirty");
}

/// Same gradient registered twice returns the same row and does
/// not re-mark dirty after a flush.
#[test]
fn register_same_gradient_twice_reuses_row() {
    let mut atlas = CpuGradientAtlas::default();
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

/// Keys differing in the smallest possible way — one stop byte, or
/// only the interpolation space — must land on different rows. The
/// index is keyed on the whole `GradientLutKey`, so this is hashbrown's
/// `Eq` doing the work rather than a hand-written confirm; the atlas
/// still owns the claim that nothing *else* distinguishes a bake.
#[test]
fn near_identical_keys_never_share_a_row() {
    let mut atlas = CpuGradientAtlas::default();
    let base = LinearGradient::two_stop(0.0, ColorU8::rgb(10, 20, 30), ColorU8::WHITE);
    let one_byte_off = LinearGradient::two_stop(0.0, ColorU8::rgb(10, 20, 31), ColorU8::WHITE);

    let mut rows = HashSet::new();
    for g in [&base, &one_byte_off] {
        for interp in [Interp::Oklab, Interp::Linear] {
            let row = atlas.register_stops(&g.stops, interp);
            assert_real_row(&atlas, row);
            assert!(rows.insert(row), "row {} aliased a distinct key", row.0);
        }
    }
    assert_eq!(rows.len(), 4);

    // And each of the four still resolves back to its own row.
    let first = atlas.register_stops(&base.stops, Interp::Oklab);
    let second = atlas.register_stops(&one_byte_off.stops, Interp::Oklab);
    assert_ne!(first, second);
}

/// Distinct gradients get distinct rows; both leave the atlas
/// dirty for upload.
#[test]
fn register_distinct_gradients_get_distinct_rows() {
    let mut atlas = CpuGradientAtlas::default();
    let _ = atlas.flush();
    let ra = register_for(&mut atlas, distinct_grad(0.1));
    let rb = register_for(&mut atlas, distinct_grad(0.2));
    assert_ne!(ra, rb);
    assert!(atlas.dirty.is_some());
}

/// Filling the atlas one distinct gradient at a time hands out every
/// real row exactly once — no key aliases another's row, and no row is
/// skipped, so the whole table is reachable.
#[test]
fn register_many_distinct_gradients_all_unique_rows() {
    let mut atlas = CpuGradientAtlas::default();
    let mut seen = HashSet::new();
    for i in 0..(INITIAL_ATLAS_ROWS - 1) {
        let g = distinct_grad(i as f32 * 0.01);
        let row = atlas.register_stops(&g.stops, g.interp);
        assert!(
            seen.insert(row),
            "row {} reused across distinct gradients",
            row.0,
        );
        assert_real_row(&atlas, row);
    }
    assert_eq!(seen.len(), INITIAL_ATLAS_ROWS as usize - 1);
}

/// Filling all 255 real slots then registering one more (after a
/// `flush`, i.e. in the next epoch) evicts the LRU row in
/// 1..INITIAL_ATLAS_ROWS — never row 0 (magenta fallback). The new gradient
/// ends up in the evicted slot; the previously resident row's
/// content hash is gone, while a surviving gradient re-registers
/// onto its exact original row (hit path).
#[test]
fn register_full_atlas_evicts_lru_and_preserves_row_zero() {
    let mut atlas = CpuGradientAtlas::default();
    let mut filled_rows: Vec<LutRow> = Vec::with_capacity((INITIAL_ATLAS_ROWS - 1) as usize);
    for i in 0..(INITIAL_ATLAS_ROWS - 1) {
        filled_rows.push(register_for(&mut atlas, distinct_grad(i as f32 * 0.01)));
    }
    // Re-touch every gradient except index 0 so the very first
    // registration's row is unambiguously the LRU.
    for i in 1..(INITIAL_ATLAS_ROWS - 1) {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    // Epoch boundary: everything above was registered "this frame"
    // and is eviction-exempt until a flush.
    let _ = atlas.flush();
    let lru = filled_rows[0];
    // Push one more distinct gradient → forces eviction.
    let evictions = atlas.counters.counts().evictions;
    let new_row = register_for(&mut atlas, distinct_grad(9999.0));
    assert_eq!(
        atlas.counters.counts().evictions,
        evictions + 1,
        "the newcomer must have displaced a resident, not taken a free row",
    );
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

/// 255 distinct registrations then a 256th in the SAME epoch grows
/// the atlas: every resident row's `LutRow` id is already captured in
/// this frame's draw payloads, so evicting one would silently paint
/// the wrong gradient. More distinct gradients than the table holds is
/// legal content, so capacity doubles and the overflow gets its own
/// row — no crash, no aliasing.
#[test]
fn full_atlas_same_epoch_overflow_grows() {
    let mut atlas = CpuGradientAtlas::default();
    let mut rows = HashSet::new();
    for i in 0..(INITIAL_ATLAS_ROWS - 1) {
        rows.insert(register_for(&mut atlas, distinct_grad(i as f32 * 0.01)));
    }
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS);

    let overflow = register_for(&mut atlas, distinct_grad(9999.0));
    assert_eq!(
        atlas.capacity(),
        INITIAL_ATLAS_ROWS * 2,
        "a full same-epoch table must double, not evict",
    );
    assert!(
        rows.insert(overflow),
        "row {} aliased a gradient this frame's draws already reference",
        overflow.0,
    );
    assert_real_row(&atlas, overflow);
    // Growth invalidates the backend's texture height, so the whole
    // atlas — not just the new rows — must re-upload.
    let flushed = atlas.flush().expect("growth must dirty the atlas");
    assert_eq!(flushed.first_row, 0);
    assert_eq!(flushed.total_rows, INITIAL_ATLAS_ROWS * 2);
    assert_eq!(
        flushed.bytes.len(),
        (INITIAL_ATLAS_ROWS * 2) as usize * size_of::<LutRowTexels>(),
    );
}

/// The hit path stamps the epoch too: re-registering all 255
/// resident gradients after a flush re-protects every row, so a
/// 256th distinct gradient in that same epoch grows rather than
/// evicting a row whose id this frame's draws already hold. The
/// re-registered gradients keep their original rows across growth.
#[test]
fn full_atlas_all_hit_this_epoch_grows() {
    let mut atlas = CpuGradientAtlas::default();
    let mut original = Vec::new();
    for i in 0..(INITIAL_ATLAS_ROWS - 1) {
        original.push(register_for(&mut atlas, distinct_grad(i as f32 * 0.01)));
    }
    let _ = atlas.flush();
    // New epoch: every row re-registered via the hit path.
    for (i, row) in original.iter().enumerate() {
        assert_eq!(
            register_for(&mut atlas, distinct_grad(i as f32 * 0.01)),
            *row,
            "hit path must reuse the resident row",
        );
    }
    let overflow = register_for(&mut atlas, distinct_grad(9999.0));
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS * 2);
    assert!(
        !original.contains(&overflow),
        "row {} aliased an epoch-protected row",
        overflow.0,
    );
}

/// Growth is bounded by the device's texture-height cap. At the cap a
/// full same-epoch table can neither evict nor grow, so the overflow
/// paints the magenta fallback: loudly wrong for that one gradient,
/// but it neither crashes nor repaints rows the frame's other draws
/// already captured. `max_rows` below the initial capacity is raised
/// to fit, so one doubling is all this atlas gets.
#[test]
fn growth_stops_at_max_rows_and_falls_back() {
    let mut atlas = CpuGradientAtlas::new(INITIAL_ATLAS_ROWS * 2);
    let mut rows = HashSet::new();
    // Fill both the initial capacity and the one doubling available.
    for i in 0..(INITIAL_ATLAS_ROWS * 2 - 1) {
        rows.insert(register_for(&mut atlas, distinct_grad(i as f32 * 0.01)));
    }
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS * 2);
    assert_eq!(rows.len(), (INITIAL_ATLAS_ROWS * 2 - 1) as usize);

    let bakes = atlas.counters.counts().bakes;
    let overflow = register_for(&mut atlas, distinct_grad(9999.0));
    assert_eq!(
        overflow,
        LutRow::FALLBACK,
        "capped atlas must fall back to magenta, not evict a live row",
    );
    assert_eq!(atlas.counters.counts().fallbacks, 1);
    assert_eq!(
        atlas.counters.counts().bakes,
        bakes,
        "a fallback must not bake — there is no row to bake into",
    );
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS * 2, "cap must hold");
    // The fallback row is still magenta — the overflow never baked
    // over it.
    let magenta = ColorF16::from(Color::linear_rgba(1.0, 0.0, 1.0, 1.0));
    assert!(atlas.baked[0].iter().all(|&t| t == magenta));

    // Next epoch: rows are evictable again, so the same gradient gets
    // a real row instead of the fallback.
    let _ = atlas.flush();
    let recovered = register_for(&mut atlas, distinct_grad(9999.0));
    assert_ne!(recovered, LutRow::FALLBACK);
    assert_real_row(&atlas, recovered);
}

/// Rows resident before a growth keep their ids AND their baked
/// content — this frame's draw payloads already hold those ids, so a
/// row moving or being rewritten under them would repaint issued draws.
///
/// Lookup goes through the key → row index, which growth doesn't
/// touch, so re-registering a resident gradient afterwards returns its
/// *original* row — the duplicate bake the open-addressed table used
/// to produce (its probe modulus moved with the capacity) is gone.
#[test]
fn growth_preserves_resident_row_content() {
    let mut atlas = CpuGradientAtlas::default();
    let pinned = distinct_grad(0.0);
    let pinned_row = register_for(&mut atlas, pinned.clone());
    let pinned_texels = atlas.baked[pinned_row.0 as usize];
    for i in 1..(INITIAL_ATLAS_ROWS - 1) {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    // Same epoch throughout, so this forces growth.
    register_for(&mut atlas, distinct_grad(9999.0));
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS * 2);
    assert_eq!(
        atlas.baked[pinned_row.0 as usize], pinned_texels,
        "growth must not disturb a row this frame's draws reference",
    );
    // Growth leaves the index alone, so this resolves to the original
    // row rather than baking a second copy of the same gradient.
    let after = register_for(&mut atlas, pinned);
    assert_eq!(after, pinned_row, "growth baked a duplicate row");
    assert_eq!(
        atlas.baked[after.0 as usize], pinned_texels,
        "the resident row's texels must survive growth intact",
    );
}

/// Hit-path bumps the row stamp: a gradient registered first, then
/// re-registered after others, must survive eviction even when the
/// table fills.
#[test]
fn register_hit_bumps_stamp_protecting_recent_content() {
    let mut atlas = CpuGradientAtlas::default();
    let pinned = distinct_grad(0.0);
    let pinned_row = register_for(&mut atlas, pinned.clone());
    // Fill 253 more rows.
    for i in 1..(INITIAL_ATLAS_ROWS - 2) {
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
    let mut atlas = CpuGradientAtlas::default();
    let first = distinct_grad(0.0);
    let _ = register_for(&mut atlas, first.clone());
    // Fill, cross the epoch boundary, then force eviction of `first`
    // (oldest stamp).
    for i in 1..(INITIAL_ATLAS_ROWS - 1) {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    let _ = atlas.flush();
    register_for(&mut atlas, distinct_grad(9999.0));
    // Re-register `first` — must succeed and return a valid row.
    let reborn = register_for(&mut atlas, first);
    assert_real_row(&atlas, reborn);
}

/// `flush` returns `Some(...)` once after a register, then `None`
/// until the next register. Idle-frame upload is zero bytes.
#[test]
fn flush_returns_bytes_once_then_none() {
    let mut atlas = CpuGradientAtlas::default();
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
    let mut atlas = CpuGradientAtlas::default();
    let stops = GradientStops::new([
        Stop::new(0.0, ColorU8::rgb(255, 64, 0)),
        Stop::new(1.0, ColorU8::rgb(0, 128, 255)),
    ]);
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
    let mut atlas = CpuGradientAtlas::default();
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
    let mut atlas = CpuGradientAtlas::default();
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

/// What the span above *costs*, reported rather than left to be
/// inferred from a byte length nothing reads back.
///
/// Two rows re-baked with a resident row between them upload three, and
/// `rows_uploaded` against `bakes` is the only place that shows. The
/// gap is what a scattered dirty set pays: the tracker is a `(min, max)`
/// pair, so it can say "rows 1 through 3" and never "rows 1 and 3".
#[test]
fn rows_uploaded_counts_the_whole_span_not_the_rows_that_changed() {
    let mut atlas = CpuGradientAtlas::default();
    let _ = atlas.flush(); // drain the magenta init row

    // Three consecutive rows, then a flush that clears the dirty range.
    let a = register_for(&mut atlas, distinct_grad(0.1));
    let b = register_for(&mut atlas, distinct_grad(0.2));
    let c = register_for(&mut atlas, distinct_grad(0.3));
    assert_eq!((a.0, b.0, c.0), (1, 2, 3), "claims walk ascending from 1");
    let _ = atlas.flush();
    let before = atlas.counters.counts();

    // Re-bake only the outer two, by evicting them: register two fresh
    // gradients after filling the table would be a bigger fixture, so
    // dirty them directly through the one path that marks rows.
    atlas.mark_row_dirty(1);
    atlas.mark_row_dirty(3);
    let f = atlas.flush().expect("two dirtied rows must flush");
    assert_eq!(f.first_row, 1);
    assert_eq!(f.bytes.len(), 3 * size_of::<LutRowTexels>());

    let delta = atlas.counters.counts() - before;
    assert_eq!(delta.bakes, 0, "nothing was re-baked, only re-uploaded");
    assert_eq!(
        delta.rows_uploaded, 3,
        "row 2 rode along because it sits between the two that changed",
    );
}

/// The invariant `register_stops` reads eviction off: rows registered
/// this epoch form a head prefix of the MRU list, so checking the tail
/// alone is equivalent to scanning for the oldest unprotected row.
///
/// Built as a genuinely mixed frame — fresh claims, hits on resident
/// rows, and rows left untouched — because the property is only
/// interesting when all three are present. Checked again after a flush
/// (the whole list becomes stale, so a vacuous all-stale prefix) and
/// after a partial re-touch in the new epoch.
#[test]
fn epoch_current_rows_form_an_mru_prefix() {
    let mut atlas = CpuGradientAtlas::default();
    for i in 0..40 {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    let _ = atlas.flush();
    assert!(atlas.epoch_prefix_holds(), "a fresh epoch protects nothing",);

    // New epoch: re-touch some resident rows out of insertion order,
    // claim some fresh ones, leave the rest alone.
    for i in [7, 31, 2, 19] {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    for i in 40..48 {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    for i in [3, 44] {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    assert!(
        atlas.epoch_prefix_holds(),
        "hits and claims must both move their row to the MRU head",
    );

    // 13 distinct rows were registered this epoch — 4 re-touched, 8
    // freshly claimed, then 3 (new) and 44 (already counted, a repeat
    // hit inside the same epoch). Pinning the count keeps the prefix
    // check above from passing vacuously on an empty prefix.
    let protected = (0..48)
        .filter(|i| {
            let g = distinct_grad(*i as f32 * 0.01);
            atlas
                .resident_row(&g.stops, g.interp)
                .is_some_and(|row| atlas.slots[row as usize].epoch == atlas.epoch)
        })
        .count();
    assert_eq!(protected, 13);
}

/// Growth leaves lookup alone: every gradient resident beforehand still
/// resolves to the row it already occupied, so no draw payload issued
/// this frame is repainted and no duplicate row is baked.
#[test]
fn growth_leaves_resident_lookups_on_their_original_rows() {
    let mut atlas = CpuGradientAtlas::default();
    let resident: Vec<LinearGradient> = (0..(INITIAL_ATLAS_ROWS - 1))
        .map(|i| distinct_grad(i as f32 * 0.01))
        .collect();
    let before: Vec<u32> = resident
        .iter()
        .map(|g| register_for(&mut atlas, g.clone()).0)
        .collect();
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS);

    // Same epoch, so the overflow has to grow rather than evict.
    register_for(&mut atlas, distinct_grad(9999.0));
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS * 2);

    let bakes = atlas.counters.counts().bakes;
    for (g, &row) in resident.iter().zip(&before) {
        assert_eq!(
            atlas.resident_row(&g.stops, g.interp),
            Some(row),
            "growth moved a resident gradient off row {row}",
        );
        assert_eq!(
            register_for(&mut atlas, g.clone()).0,
            row,
            "re-registering after growth baked a duplicate instead of \
             resolving to row {row}",
        );
    }
    assert_eq!(
        atlas.counters.counts().bakes,
        bakes,
        "re-registering after growth baked at all — the open-addressed \
         table used to duplicate here because its probe modulus moved",
    );
    // A duplicate bake would have consumed rows beyond the 255 resident
    // ones plus the overflow.
    assert_eq!(atlas.index_len(), INITIAL_ATLAS_ROWS as usize);
}

/// Eviction takes the outgoing gradient out of the index with its row.
/// Leaving it behind is the failure mode unique to splitting lookup
/// from storage: the stale entry would resolve to a row now holding
/// somebody else's bake, and the evicted gradient would paint the
/// wrong colours instead of re-baking.
#[test]
fn eviction_drops_the_outgoing_key_from_the_index() {
    let mut atlas = CpuGradientAtlas::default();
    let first = distinct_grad(0.0);
    let first_row = register_for(&mut atlas, first.clone()).0;
    for i in 1..(INITIAL_ATLAS_ROWS - 1) {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    let _ = atlas.flush();

    // `first` is the least-recently-registered, so it is the victim.
    let newcomer = distinct_grad(9999.0);
    assert_eq!(register_for(&mut atlas, newcomer.clone()).0, first_row);
    assert_eq!(
        atlas.resident_row(&first.stops, first.interp),
        None,
        "evicted gradient still resolves to a row",
    );
    assert_eq!(
        atlas.resident_row(&newcomer.stops, newcomer.interp),
        Some(first_row),
    );
    // The table stayed at one entry per occupied row.
    assert_eq!(atlas.index_len(), (INITIAL_ATLAS_ROWS - 1) as usize);

    // Re-registering the evicted content re-bakes it somewhere else,
    // and its texels are the real gradient rather than the newcomer's.
    let _ = atlas.flush();
    let reborn = register_for(&mut atlas, first.clone()).0;
    assert_ne!(reborn, first_row);
    let mut expected = fresh_row();
    bake_stops(&first.stops, first.interp, &mut expected);
    assert_eq!(atlas.baked[reborn as usize], expected);
}

/// The row ceiling is the *policy* cap, not the device's texture
/// limit: growth never reverses, so a 16384-row adapter would let one
/// pathological frame pin 32 MB for the life of the process.
#[test]
fn shared_atlas_clamps_device_limit_to_the_policy_cap() {
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    use std::num::NonZeroU32;

    let huge = SharedGradientAtlas::new(TextureLimit::from_device(NonZeroU32::new(16384).unwrap()));
    assert_eq!(huge.max_rows(), MAX_ATLAS_ROWS);
    // A device below the cap still binds.
    let small = SharedGradientAtlas::new(TextureLimit::from_device(NonZeroU32::new(1024).unwrap()));
    assert_eq!(small.max_rows(), 1024);
    // Deviceless keeps the conservative downlevel fallback.
    assert_eq!(
        SharedGradientAtlas::new(TextureLimit::default()).max_rows(),
        DEFAULT_MAX_ATLAS_ROWS,
    );
}

/// The headline steady-state property: a frame redrawing unchanged
/// gradients bakes nothing. Every registration resolves from the index,
/// nothing is evicted, and the atlas holds its size.
///
/// This is what the cache is *for*, and before the probe existed there
/// was no way to tell it from a cache that re-baked every row and
/// happened to return the same ids.
#[test]
fn steady_state_frames_never_rebake() {
    const GRADIENTS: u32 = 64;
    const FRAMES: u32 = 10;

    let mut atlas = CpuGradientAtlas::default();
    let content: Vec<_> = (0..GRADIENTS)
        .map(|i| distinct_grad(i as f32 * 0.01))
        .collect();
    let rows: Vec<LutRow> = content
        .iter()
        .map(|g| register_for(&mut atlas, g.clone()))
        .collect();
    let after_warmup = atlas.counters.counts().bakes;
    assert_eq!(after_warmup, GRADIENTS);

    for _ in 0..FRAMES {
        atlas.flush();
        for (g, &row) in content.iter().zip(&rows) {
            assert_eq!(
                register_for(&mut atlas, g.clone()),
                row,
                "steady-state frame moved a gradient off its row",
            );
        }
    }

    let counts = atlas.counters.counts();
    assert_eq!(
        counts.bakes, after_warmup,
        "a steady-state frame must not bake",
    );
    assert_eq!(counts.evictions, 0);
    assert_eq!(counts.growths, 0);
    assert_eq!(counts.hits, GRADIENTS * FRAMES);
    assert_eq!(
        counts.registrations,
        GRADIENTS * (FRAMES + 1),
        "warm-up misses plus every frame's hits",
    );
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS);
}

/// Churn across epochs evicts; it must never grow.
///
/// This is the ratchet guard. Growth is one-way — the atlas has no
/// shrink path — so a workload that grows the table when it should have
/// evicted permanently enlarges every structure the register path
/// touches. A gradient animated per frame produces exactly this:
/// a working set far larger than the table, none of it reused.
///
/// Cycling a set twice the table's size is LRU's worst case by
/// construction, so every registration here is a miss. That is the
/// point — it is the shape most likely to trip a grow-instead-of-evict
/// bug, and each round crosses an epoch boundary the way a real frame
/// does.
#[test]
fn cross_epoch_churn_evicts_without_growing() {
    let working_set = (INITIAL_ATLAS_ROWS * 2) as usize;
    let mut atlas = CpuGradientAtlas::default();
    let content: Vec<_> = (0..working_set)
        .map(|i| distinct_grad(i as f32 * 0.01))
        .collect();

    for round in 0..4 {
        for g in &content {
            atlas.flush();
            let row = register_for(&mut atlas, g.clone());
            assert_real_row(&atlas, row);
        }
        assert_eq!(
            atlas.capacity(),
            INITIAL_ATLAS_ROWS,
            "round {round} grew the atlas instead of evicting",
        );
    }

    let registrations = (working_set * 4) as u32;
    let counts = atlas.counters.counts();
    assert_eq!(counts.registrations, registrations);
    assert_eq!(counts.growths, 0);
    // Cyclic access over 2x the table never reuses a resident row, so
    // every registration misses; the first INITIAL_ATLAS_ROWS - 1 take
    // never-claimed rows and the rest evict.
    assert_eq!(counts.hits, 0, "cyclic churn cannot hit");
    assert_eq!(counts.bakes, registrations);
    assert_eq!(counts.evictions, registrations - (INITIAL_ATLAS_ROWS - 1),);
    assert_eq!(atlas.index_len(), (INITIAL_ATLAS_ROWS - 1) as usize);
}

/// A miss bakes exactly one row — never two.
///
/// The old table could bake a resident gradient a second time after a
/// growth moved its probe base, which showed up only as a quietly
/// wasted row. Pinning bakes against misses makes any repeat bake a
/// failure rather than a slow leak.
#[test]
fn every_miss_bakes_exactly_one_row() {
    let mut atlas = CpuGradientAtlas::default();
    // Mixed traffic: fresh content, immediate repeats, and repeats of
    // content registered several steps back.
    let content: Vec<_> = (0..40).map(|i| distinct_grad(i as f32 * 0.01)).collect();
    let sequence: Vec<usize> = (0..40).chain(0..40).chain([3, 3, 17, 39, 0]).collect();

    let mut expected_bakes = 0u32;
    let mut seen = HashSet::new();
    for &i in &sequence {
        if seen.insert(i) {
            expected_bakes += 1;
        }
        register_for(&mut atlas, content[i].clone());
    }

    let counts = atlas.counters.counts();
    assert_eq!(counts.bakes, expected_bakes);
    assert_eq!(counts.bakes, 40, "each distinct gradient baked once");
    assert_eq!(
        counts.hits,
        sequence.len() as u32 - expected_bakes,
        "every non-first occurrence must resolve from the index",
    );
    assert_eq!(counts.evictions, 0, "40 gradients fit in 255 rows");
}
