// Rings that run out from the center and bend what is under them along their slope, as water
// over a picture does; the bend reads at most `amplitude` px away, the shape's reach.

struct Params {
    tint: vec4<f32>,
    amplitude: f32,
    wavelength: f32,
    pad0: f32,
    pad1: f32,
}

const TURN: f32 = 6.2831853;
// Two rings pass a point each 4 s loop, so the loop joins with no seam.
const LOOP_S: f32 = 4.0;

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    let center = input.size * 0.5;
    let from_center = input.pos - center;
    let d = length(from_center);
    let phase = d * TURN / params.wavelength - input.time * 2.0 * TURN / LOOP_S;
    // Bent along the radius; at the center, where it has no direction, by nothing.
    let dir = select(from_center / d, vec2<f32>(0.0), d == 0.0);
    let under = effect_backdrop(input.pos + dir * sin(phase) * params.amplitude);
    let shade = 1.0 + 0.1 * cos(phase);
    let color = mix(under.rgb * shade, params.tint.rgb, params.tint.a);
    return EffectOutput(vec4<f32>(color, 1.0), vec4<f32>(0.0));
}
