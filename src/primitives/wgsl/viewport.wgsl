// The frame's coordinates: the immediate region every palantir shader reads, and the two maps
// between the surface's physical px and a shader's positions.

// The whole immediate region: the viewport size and the fragment origin, which the backend pushes
// after every pipeline bind (`ViewportPush`). Every pipeline layout declares its size,
// `IMMEDIATES_BYTES`.
struct PalantirImmediates {
    viewport_size: vec2<f32>,
    // The surface pixel the target's pixel 0 is: zero, but on a damage canvas, which holds a rect
    // of the surface.
    frag_origin: vec2<f32>,
}
var<immediate> palantir_imm: PalantirImmediates;

// Physical px to a clip-space position. The y lane flips, as the pixel grid runs down from the
// top-left and clip space up from the center. The viewport is read here rather than passed in, so
// two shaders cannot answer this against different ones.
fn palantir_clip_from_px(px: vec2<f32>) -> vec4<f32> {
    let ndc = px * (vec2<f32>(2.0, -2.0) / palantir_imm.viewport_size) + vec2<f32>(-1.0, 1.0);
    return vec4<f32>(ndc, 0.0, 1.0);
}

// A fragment's position in the surface's physical px, whatever target it paints into:
// `@builtin(position)` counts from the target's pixel 0.
fn palantir_frag_px(clip: vec4<f32>) -> vec2<f32> {
    return clip.xy + palantir_imm.frag_origin;
}
