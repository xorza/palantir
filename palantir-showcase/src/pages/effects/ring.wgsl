// A glow around a rounded card that sits `inset` px inside the effect's rect: its color turns
// around the card once a loop, and it fades with the distance from the card's edge, to nothing at
// the rect's.

struct Params {
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    inset: f32,
    radius: f32,
    glow: f32,
    pad0: f32,
}

const TURN: f32 = 6.2831853;
const LOOP_S: f32 = 6.0;

// The signed distance from `p` to a box of half size `half` and corner radius `r` about the
// origin, negative inside.
fn rounded_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    let p = input.pos - input.size * 0.5;
    let d = rounded_box(p, input.size * 0.5 - vec2<f32>(params.inset), params.radius);
    let outside = max(d, 0.0);
    let band = saturate(1.5 - abs(d - 1.0) * input.scale);
    // Faded to nothing at the rect's edge, `inset` px out, so the rect shows no seam.
    let falloff = exp(-outside / params.glow) * saturate(1.0 - outside / params.inset);
    let glow = max(falloff * select(0.0, 0.8, d > 0.0), band);
    let k = 0.5 + 0.5 * sin(atan2(p.y, p.x) + input.time * TURN / LOOP_S);
    let color = mix(params.color_a, params.color_b, k);
    let a = color.a * glow;
    return effect_over(vec4<f32>(color.rgb * a, a));
}
