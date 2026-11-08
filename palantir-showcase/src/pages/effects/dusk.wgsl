// The aurora's still stand-in under the power saver: one vertical blend of its two colors.

struct Params {
    low: vec4<f32>,
    high: vec4<f32>,
}

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    let color = mix(params.low, params.high, input.pos.y / input.size.y);
    return effect_over(vec4<f32>(color.rgb * color.a, color.a));
}
