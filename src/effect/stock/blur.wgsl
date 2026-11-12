// The stock blur: what the material reads, blurred by its capture's σ, as it is.

fn effect(input: EffectInput) -> EffectOutput {
    return effect_over(effect_backdrop(input.pos));
}
