// The stock frosted glass: what the material reads, saturated and grained (`FrostedInput`).

struct Params {
    saturation: f32,
    noise: f32,
}

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    return effect_over(vec4<f32>(material_frost(effect_backdrop(input.pos).rgb, input, params.saturation, params.noise), 1.0));
}
