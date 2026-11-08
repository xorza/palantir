
// Palantir's stages around `effect`. Every name here carries the `palantir_` prefix, which an
// effect's own text leaves to palantir.

struct PalantirImmediates {
    viewport_size: vec2<f32>,
    frag_origin: vec2<f32>,
}
var<immediate> palantir_imm: PalantirImmediates;

// Every instance's parameters, as 32-bit words; an instance reads from its own offset.
@group(0) @binding(0) var<storage, read> palantir_words: array<u32>;

const PALANTIR_AA_HALF_WIDTH: f32 = /*{AA_HALF_WIDTH}*/;

const PALANTIR_CORNERS = array<vec2<f32>, 4>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 1.0),
);

// One shape: its rect (`min`, `size`) and corner radii in physical px, its clock (`time`,
// `opacity`, `scale`) and the word its parameters start at.
struct PalantirInstance {
    @location(0) rect: vec4<f32>,
    @location(1) corners: vec4<f32>,
    @location(2) clock: vec3<f32>,
    @location(3) params: u32,
}

struct PalantirVarying {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) rect: vec4<f32>,
    @location(1) @interpolate(flat) corners: vec4<f32>,
    @location(2) @interpolate(flat) clock: vec3<f32>,
    @location(3) @interpolate(flat) params: u32,
}

struct PalantirOut {
    @location(0) @blend_src(0) color: vec4<f32>,
    @location(0) @blend_src(1) under: vec4<f32>,
}

// The rect grown by the antialiasing reach, so every pixel the edge's ramp touches draws.
@vertex
fn palantir_vs(@builtin(vertex_index) vi: u32, instance: PalantirInstance) -> PalantirVarying {
    let reach = vec2<f32>(PALANTIR_AA_HALF_WIDTH);
    let px = instance.rect.xy - reach + PALANTIR_CORNERS[vi] * (instance.rect.zw + 2.0 * reach);
    let ndc = px * (vec2<f32>(2.0, -2.0) / palantir_imm.viewport_size) + vec2<f32>(-1.0, 1.0);
    return PalantirVarying(
        vec4<f32>(ndc, 0.0, 1.0),
        instance.rect,
        instance.corners,
        instance.clock,
        instance.params,
    );
}

// The rounded rect's signed distance at `p` from its top-left, radii fitted on the CPU.
fn palantir_sdf(p: vec2<f32>, size: vec2<f32>, radius: vec4<f32>) -> f32 {
    let half = size * 0.5;
    let c = p - half;
    let right = step(0.0, c.x);
    let bottom = step(0.0, c.y);
    let r = mix(mix(radius.x, radius.y, right), mix(radius.w, radius.z, right), bottom);
    let q = abs(c) - (half - vec2<f32>(r));
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
}
/*{PARAMS}*/
@fragment
fn palantir_fs(v: PalantirVarying) -> PalantirOut {
    let local = v.clip.xy + palantir_imm.frag_origin - v.rect.xy;
    let d = palantir_sdf(local, v.rect.zw, v.corners);
    let coverage = clamp((PALANTIR_AA_HALF_WIDTH - d) / (2.0 * PALANTIR_AA_HALF_WIDTH), 0.0, 1.0)
        * v.clock.y;
    let scale = v.clock.z;
    let input = EffectInput(local / scale, v.rect.zw / scale, v.corners / scale, v.clock.x, scale);
    let out = /*{CALL}*/;
    return PalantirOut(out.color * coverage, vec4<f32>(1.0) - coverage * (vec4<f32>(1.0) - out.under));
}
