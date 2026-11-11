// A halo around a rounded card: its color turns around the card once a loop, and it fades with
// the distance from the card's edge, to nothing at the halo's reach.

struct Params {
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    reach: f32,
    glow: f32,
    pad0: f32,
    pad1: f32,
}

const TURN: f32 = 6.2831853;
const LOOP_S: f32 = 6.0;

// The signed distance from `p` to a box of half size `half` about the origin, with the corner
// radii of `corners` (top-left, top-right, bottom-right, bottom-left, y down), negative inside.
fn rounded_box(p: vec2<f32>, half: vec2<f32>, corners: vec4<f32>) -> f32 {
    let side = select(corners.xw, corners.yz, p.x > 0.0);
    let r = select(side.x, side.y, p.y > 0.0);
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    let p = input.pos - input.size * 0.5;
    let d = max(rounded_box(p, input.size * 0.5, input.corners), 0.0);
    let band = saturate(1.5 - abs(d - 1.0) * input.scale);
    // Faded to nothing at the reach, where the halo stops, so it shows no seam.
    let falloff = exp(-d / params.glow) * saturate(1.0 - d / params.reach);
    let glow = max(falloff * 0.8, band);
    let k = 0.5 + 0.5 * sin(atan2(p.y, p.x) + input.time * TURN / LOOP_S);
    let color = mix(params.color_a, params.color_b, k);
    let a = color.a * glow;
    return effect_over(vec4<f32>(color.rgb * a, a));
}
