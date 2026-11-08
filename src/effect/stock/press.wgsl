// What both stock press effects read, palantir's `PressInput`, and the timing they share.

struct Params {
    color: vec4<f32>,
    point: vec2<f32>,
    released: f32,
    enter: f32,
    exit: f32,
    seed: f32,
    pad0: f32,
    pad1: f32,
}

// How far `time` is through a span of `span` seconds that starts at 0, in [0, 1]; a span of 0
// is through at once.
fn press_through(time: f32, span: f32) -> f32 {
    if (span <= 0.0) {
        return 1.0;
    }
    return saturate(time / span);
}

// How far the fade-out is, in [0, 1]: 0 while the press is held, then over `exit` from the
// release or the end of `enter`, whichever is later, as `PressInput` times it.
fn press_fade(params: Params, time: f32) -> f32 {
    if (params.released < 0.0) {
        return 0.0;
    }
    return press_through(time - max(params.released, params.enter), params.exit);
}

// `cubic-bezier(0.4, 0, 0.2, 1)`, Material's FastOutSlowIn, at `u` in [0, 1]: Newton's method
// on the curve's x from s = u, then its y. The x slope stays at or above 0.525, so five steps
// leave an x error under 3e-9, below an f32 step at 1.
fn press_fast_out_slow_in(u: f32) -> f32 {
    var s = u;
    for (var i = 0; i < 5; i++) {
        let r = 1.0 - s;
        let x = 1.2 * r * r * s + 0.6 * r * s * s + s * s * s;
        let slope = 1.2 * r * r - 1.2 * r * s + 2.4 * s * s;
        s = saturate(s - (x - u) / slope);
    }
    return s * s * (3.0 - 2.0 * s);
}
