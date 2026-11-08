// Two colors swept by two sine waves that cross; every speed is a whole number of turns over the
// 20 s loop, so the loop joins with no seam.

struct Params {
    low: vec4<f32>,
    high: vec4<f32>,
}

const TURN: f32 = 6.2831853;
const LOOP_S: f32 = 20.0;

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    let uv = input.pos / input.size;
    let spin = input.time * TURN / LOOP_S;
    let across = sin(uv.x * 5.0 + spin * 3.0);
    let down = sin(uv.y * 3.0 + uv.x * 2.0 - spin * 2.0);
    let k = saturate(0.5 + 0.25 * (across + down));
    let color = mix(params.low, params.high, k);
    return effect_over(vec4<f32>(color.rgb * color.a, color.a));
}
