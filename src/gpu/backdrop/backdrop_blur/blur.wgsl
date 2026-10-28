// The backdrop blur's passes: the `[1 3 3 1]` tent that halves a level, and
// the two axes of the residual Gaussian. See `BlurPlan`.
//
// Every read is a `textureLoad` with the weights in the shader: a bilinear
// fetch between two texels splits its weight at the hardware's sub-texel
// precision, which Vulkan lets be as coarse as 1/16, and a level's texels
// can differ by the whole contrast of the content under a pane. Integer
// reads also make a pass the same arithmetic wherever its target sits.
//
// Coordinates are texels of the surface's lattice at the level read: texel
// `i` of level `k` covers physical px `i·2^k .. (i+1)·2^k`. A pass maps the
// texel it writes onto the lattice through `target_to_lattice`, mirrors past
// the level's edge, which is the surface's, and reads the source texture at
// the lattice texel minus `source_origin`.

// Vectors of four the residual's taps fill: `BlurPlan::MAX_TAPS` of them.
const TAP_VEC4S: u32 = /*{TAP_VEC4S}*/;

struct BlurParams {
    // Added to the texel a fragment writes, gives its lattice texel.
    target_to_lattice: vec2<i32>,
    // The extent of the level read, in its texels: where the mirror folds.
    level_size: vec2<i32>,
    // The lattice texel the source texture's first texel holds.
    source_origin: vec2<i32>,
    // The source texture's last texel; a read outside a result's support
    // lands anywhere inside it.
    source_max: vec2<i32>,
    // The residual's radius; its taps sit four to a vector.
    radius: i32,
    taps: array<vec4<f32>, TAP_VEC4S>,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(1) @binding(0) var<uniform> params: BlurParams;

// One triangle over the whole target; the scissor picks the texels written.
@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

// The texel `i` folds to on a level of `n` texels: past an edge the level is
// mirrored with the edge texel repeated, as `MirrorRepeat` addresses.
fn mirror(i: i32, n: i32) -> i32 {
    let period = 2 * n;
    let m = ((i % period) + period) % period;
    return select(m, period - 1 - m, m >= n);
}

fn mirrored(t: vec2<i32>) -> vec2<i32> {
    return vec2<i32>(mirror(t.x, params.level_size.x), mirror(t.y, params.level_size.y));
}

fn load(lattice: vec2<i32>) -> vec4<f32> {
    let t = clamp(lattice - params.source_origin, vec2<i32>(0), params.source_max);
    return textureLoad(source, t, 0);
}

fn tap(k: i32) -> f32 {
    let i = u32(abs(k));
    return params.taps[i / 4u][i % 4u];
}

fn lattice_texel(position: vec4<f32>) -> vec2<i32> {
    return vec2<i32>(position.xy) + params.target_to_lattice;
}

// A texel of the next level: the source texels at offsets ±½ and ±1½ of its
// center, weighted `[1 3 3 1] / 8` on each axis.
@fragment
fn fs_tent(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let base = lattice_texel(position) * 2 - vec2<i32>(1);
    let weights = array<f32, 4>(0.125, 0.375, 0.375, 0.125);
    var sum = vec4<f32>(0.0);
    for (var j = 0; j < 4; j++) {
        var row = vec4<f32>(0.0);
        for (var i = 0; i < 4; i++) {
            row += weights[i] * load(mirrored(base + vec2<i32>(i, j)));
        }
        sum += weights[j] * row;
    }
    return sum;
}

@fragment
fn fs_horizontal(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let at = mirrored(lattice_texel(position));
    var sum = vec4<f32>(0.0);
    for (var k = -params.radius; k <= params.radius; k++) {
        sum += tap(k) * load(vec2<i32>(mirror(at.x + k, params.level_size.x), at.y));
    }
    return sum;
}

@fragment
fn fs_vertical(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let at = mirrored(lattice_texel(position));
    var sum = vec4<f32>(0.0);
    for (var k = -params.radius; k <= params.radius; k++) {
        sum += tap(k) * load(vec2<i32>(at.x, mirror(at.y + k, params.level_size.y)));
    }
    return sum;
}
