
// The effect contract: what an effect reads and returns, and its blends. Positions and lengths
// are logical px; colors are linear, premultiplied.

struct EffectInput {
    // The pixel's center from the effect rect's top-left.
    pos: vec2<f32>,
    // The effect rect's size.
    size: vec2<f32>,
    // The rect's corner radii: top-left, top-right, bottom-right, bottom-left.
    corners: vec4<f32>,
    // Seconds into the pass of the shape's paint animation; 0 without one.
    time: f32,
    // Physical px per logical px.
    scale: f32,
}

// What an effect paints: `color` added to what is under it weighted by `under`, per channel:
// `color + under · dst`, with the shape's coverage applied exactly.
struct EffectOutput {
    color: vec4<f32>,
    under: vec4<f32>,
}

// `color` over what is under it.
fn effect_over(color: vec4<f32>) -> EffectOutput {
    return EffectOutput(color, vec4<f32>(1.0 - color.a));
}

// `color` added to what is under it.
fn effect_add(color: vec4<f32>) -> EffectOutput {
    return EffectOutput(color, vec4<f32>(1.0));
}

// What is under it multiplied by `color`, as `color` lies over it: `dst · (rgb + 1 − α)`.
fn effect_multiply(color: vec4<f32>) -> EffectOutput {
    return EffectOutput(vec4<f32>(0.0), vec4<f32>(color.rgb + vec3<f32>(1.0 - color.a), 1.0));
}

// What is under it screened by `color`: `color + dst · (1 − color)`.
fn effect_screen(color: vec4<f32>) -> EffectOutput {
    return EffectOutput(color, vec4<f32>(1.0) - color);
}

// `p` turned `rotation` radians about `center`, exact at no turn: palantir's, as its stages and
// `effect_backdrop` turn a shape's frame by it.
fn palantir_turned(p: vec2<f32>, center: vec2<f32>, rotation: f32) -> vec2<f32> {
    if (rotation == 0.0) {
        return p;
    }
    let cs = vec2<f32>(cos(rotation), sin(rotation));
    let v = p - center;
    return center + vec2<f32>(cs.x * v.x - cs.y * v.y, cs.y * v.x + cs.x * v.y);
}
