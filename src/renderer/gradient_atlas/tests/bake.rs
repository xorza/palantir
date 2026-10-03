//! What one row's texels come out as: interpolation space, stop order, and
//! edge clamping.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and colour channels"
)]

use crate::primitives::math::approx;
use crate::primitives::paint::brush::gradient::Interp;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::paint::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::oklab;
use crate::renderer::gradient_atlas::tests::support::fresh_row;
use crate::renderer::gradient_atlas::*;
use std::array;
use std::collections::HashSet;

/// `Interp::Linear`: midpoint of black→white in linear-RGB space
/// is exactly linear 0.5. The sampler reads the f16 store directly
/// as the linear value the shader uses. Regression check: an
/// accidental sRGB-space lerp would produce linear ≈ 0.215, far
/// below the 0.4 threshold.
#[test]
fn linear_midpoint_black_to_white_is_half() {
    let g =
        LinearGradient::two_stop(0.0, RgbaF32::BLACK, RgbaF32::WHITE).with_interp(Interp::Linear);
    let mut out = fresh_row();
    bake::row(&g.ramp, &mut out);
    // Texel 127 sits at t = 127/255, so the grey is that, f16-rounded.
    let t = 127.0 / 255.0;
    assert_eq!(texel(&out, 127), stored(RgbaF32::new(t, t, t, 1.0)));
}

/// `Interp::Oklab`: red→green midpoint should *not* be muddy
/// brown (which is what linear-RGB lerps produce). Specifically,
/// the green channel at midpoint should be high (Oklab keeps
/// luminance up through the midpoint by traversing yellow-ish
/// hues rather than dipping through dark brown).
#[test]
fn oklab_red_to_green_midpoint_avoids_muddy_brown() {
    let red = linear(255, 0, 0);
    let green = linear(0, 255, 0);
    let g = LinearGradient::two_stop(0.0, red, green).with_interp(Interp::Oklab);
    let mut out = fresh_row();
    bake::row(&g.ramp, &mut out);
    // The midpoint is the two stops' Oklab coordinates blended at
    // t = 127/255 and brought back to linear — a warm yellow, where a
    // linear-RGB blend dips through dark brown.
    let t = 127.0 / 255.0;
    let (from, to) = (
        oklab::from_linear(1.0, 0.0, 0.0),
        oklab::from_linear(0.0, 1.0, 0.0),
    );
    let [r, g, b] = oklab::to_linear(array::from_fn(|i| from[i] + (to[i] - from[i]) * t));
    let mid = texel(&out, 127);
    assert_stored(mid, RgbaF32::new(r, g, b, 1.0), "Oklab midpoint");
}

/// First and last texels hold the corresponding stops' stored colours.
/// Catches off-by-one in the parametric `t = i/(N-1)` stride and the
/// edge-clamp guard.
#[test]
fn endpoints_match_stops_exactly() {
    for interp in [Interp::Linear, Interp::Oklab] {
        let g = LinearGradient::two_stop(0.0, linear(11, 22, 33), linear(244, 233, 222))
            .with_interp(interp);
        let mut out = fresh_row();
        bake::row(&g.ramp, &mut out);
        let label = format!("interp={interp:?}");
        assert_stored(texel(&out, 0), g.ramp.stops[0].color(), &label);
        assert_stored(
            texel(&out, LUT_ROW_TEXELS - 1),
            g.ramp.stops[1].color(),
            &label,
        );
    }
}

/// 3-stop gradient at offset `0.25` falls in the first half of the
/// `0.0..0.5` bracket — should be halfway between stop 0 and stop
/// 1, not stop 1 and stop 2. Catches bracketing logic.
#[test]
fn three_stop_quarter_brackets_first_pair() {
    let g = LinearGradient::builder(0.0)
        .stop(0.0, linear(0, 0, 0))
        .stop(0.5, linear(255, 0, 0))
        .stop(1.0, linear(0, 0, 255))
        .with_interp(Interp::Linear)
        .build();
    let mut out = fresh_row();
    bake::row(&g.ramp, &mut out);
    // Stop offsets snap to the texel grid, so the 0.5 stop sits on texel
    // 128 (offset 128/255). Texel 64 is then exactly halfway into the
    // first segment, 64/128 of the way from black to red.
    assert_eq!(g.ramp.stops[1].offset(), 128.0 / 255.0);
    let q = texel(&out, 64);
    assert_eq!(q, stored(RgbaF32::new(0.5, 0.0, 0.0, 1.0)));
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
    fn restart_scan(stops: &GradientStops, t: f32) -> RgbaF32 {
        let linear: Vec<RgbaF32> = stops.iter().map(|stop| stop.color()).collect();
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
        if approx::approx_zero(denominator) {
            return linear[upper];
        }
        RgbaF32::lerp(
            linear[upper - 1],
            linear[upper],
            (t - lower_offset) / denominator,
        )
    }

    let g = LinearGradient::new(
        0.0,
        [
            Stop::new(0.0, linear(0, 0, 0)),
            Stop::new(0.002, linear(255, 0, 0)), // narrower than one texel
            Stop::new(0.25, linear(0, 255, 0)),
            Stop::new(0.5, linear(0, 0, 255)),
            Stop::new(0.5, linear(255, 255, 0)), // hard stop
            Stop::new(0.75, linear(0, 255, 255)),
            Stop::new(0.9, linear(255, 0, 255)),
            Stop::new(1.0, linear(255, 255, 255)),
        ],
    )
    .with_interp(Interp::Linear);
    let mut out = fresh_row();
    bake::row(&g.ramp, &mut out);
    for (i, got) in out.iter().enumerate() {
        let t = i as f32 / (LUT_ROW_TEXELS - 1) as f32;
        let want = RgbaF16::from(restart_scan(&g.ramp.stops, t));
        assert_eq!(*got, want, "texel {i} at t={t}");
    }
}

/// Pin the row layout: 256 `RgbaF16` texels = 2048 bytes total,
/// `[r, g, b, a]` f16 lanes per texel. Endpoint texels decode back
/// to their stops' linear values.
#[test]
fn lut_row_layout() {
    assert_eq!(LUT_ROW_TEXELS, 256);
    assert_eq!(size_of::<LutRowTexels>(), 2048);
    assert_eq!(size_of::<RgbaF16>(), 8);
    let g = LinearGradient::two_stop(0.0, linear(1, 2, 3), linear(4, 5, 6));
    let mut out = fresh_row();
    bake::row(&g.ramp, &mut out);
    assert_stored(texel(&out, 0), g.ramp.stops[0].color(), "first");
    assert_stored(
        texel(&out, LUT_ROW_TEXELS - 1),
        g.ramp.stops[1].color(),
        "last",
    );
}

/// Unsorted stops are sorted at bake time. Authors shouldn't rely
/// on this — `LinearGradient::new` accepts any order — but the
/// bake must produce a sensible output regardless.
#[test]
fn unsorted_stops_get_sorted_at_bake() {
    let stops = [
        Stop::new(1.0, linear(255, 0, 0)), // out of order
        Stop::new(0.0, linear(0, 0, 255)),
    ];
    let g = LinearGradient::new(0.0, stops);
    let mut out = fresh_row();
    bake::row(&g.ramp, &mut out);
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
        Stop::new(0.25, linear(0, 255, 0)),
        Stop::new(0.75, linear(0, 0, 255)),
    ];
    let g = LinearGradient::new(0.0, stops);
    let mut out = fresh_row();
    bake::row(&g.ramp, &mut out);
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
    let navy = RgbaF32::hex(0x1a1a2e);
    let blue = RgbaF32::hex(0x4c5cdb);
    // The whole problem: both stops linearise to tiny reds, 2.6/255 and
    // 18.4/255, so the bake walks a narrow span that an 8-bit linear row
    // can't resolve.
    assert_eq!([navy.r, blue.r], [0.010329823, 0.07227185]);
    let g = LinearGradient::two_stop(0.0, navy, blue); // default Oklab
    let mut out = fresh_row();
    bake::row(&g.ramp, &mut out);

    let reds: Vec<f32> = (0..LUT_ROW_TEXELS).map(|i| texel(&out, i).r).collect();

    // f16 store: per-texel red delta (~2.5e-4) dwarfs the f16 ulp
    // (~8e-6) at this magnitude, so every texel holds its own red.
    let distinct_f16 = reds
        .iter()
        .map(|r| r.to_bits())
        .collect::<HashSet<_>>()
        .len();
    assert_eq!(
        distinct_f16, LUT_ROW_TEXELS,
        "f16 red: a distinct level at every texel"
    );

    // Counterfactual: the old `Rgba8Unorm` store quantized these
    // same reds to 8-bit linear, collapsing onto ≤ 20 levels.
    let distinct_u8 = reds
        .iter()
        .map(|r| (r * 255.0).round() as u8)
        .collect::<HashSet<_>>()
        .len();
    // 8-bit linear: the reds span 3..=18, sixteen levels across 256 texels.
    assert_eq!(distinct_u8, 16, "premise: 8-bit linear bands hard");
}

/// What the f16 store holds for `color`: its rounding, decoded.
fn stored(color: RgbaF32) -> RgbaF32 {
    RgbaF16::from(color).unpack()
}

/// One baked texel decoded back to a linear `RgbaF32`. The f16 store
/// round-trips losslessly enough that `≈` comparisons hold to well
/// under a u8 LSB (1/255).
fn texel(out: &LutRowTexels, i: usize) -> RgbaF32 {
    out[i].unpack()
}

/// An opaque stop colour whose channels are `byte / 255` in linear
/// light, with no sRGB decode.
fn linear(r: u8, g: u8, b: u8) -> RgbaF32 {
    let lin = |byte: u8| f32::from(byte) / 255.0;
    RgbaF32::new(lin(r), lin(g), lin(b), 1.0)
}

/// Assert a baked texel holds `want` as the f16 store rounds it.
fn assert_stored(got: RgbaF32, want: RgbaF32, what: &str) {
    for (chan, got, want) in [
        ("r", got.r, want.r),
        ("g", got.g, want.g),
        ("b", got.b, want.b),
        ("a", got.a, want.a),
    ] {
        let stored = half::f16::from_f32(want).to_f32();
        assert_eq!(got, stored, "{what} {chan}: want {want}");
    }
}
