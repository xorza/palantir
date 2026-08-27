//! What one row's texels come out as: interpolation space, stop order, and
//! edge clamping.

use crate::primitives::brush::gradient::Interp;
use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::color::ColorU8;
use crate::renderer::gradient_atlas::tests::support::fresh_row;
use crate::renderer::gradient_atlas::*;
use std::collections::HashSet;

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
    // Stops 0 and 1 are both b=0, so the whole first segment bakes b=0 —
    // stop 2's b=1.0 is not reached until past the midpoint.
    assert_eq!(q.b, 0.0, "quarter-texel leaked blue from stop 2");
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
