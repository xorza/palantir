// The backdrop blur's passes: the `[1 3 3 1]` tent that halves a level, and
// the two axes of the residual Gaussian. See `BlurPlan`.
//
// A residual tap is a `textureLoad` with its weight in the shader: a
// bilinear fetch between two texels splits its weight at the hardware's
// sub-texel precision, which Vulkan lets be as coarse as 1/16, and a level's
// texels can differ by the whole contrast of the content under a pane. A
// tent's fetch splits a quarter from three quarters, which 1/16 holds
// exactly. Either way a read lands on the lattice's texels at a fixed phase,
// so a pass is the same arithmetic wherever its target sits.
//
// Coordinates are texels of the surface's lattice at the level read: texel
// `i` of level `k` covers physical px `i·2^k .. (i+1)·2^k`. A pass maps the
// texel it writes onto the lattice through `target_to_lattice`, mirrors past
// the level's edge, which is the surface's, and reads the source texture at
// the lattice texel minus `source_origin`.

// Vectors of four the residual's taps fill: `BlurPlan::MAX_TAPS` of them.
const TAP_VEC4S: u32 = /*{TAP_VEC4S}*/;

// Whether the residual runs one loop that mirrors each read where its window crosses the
// level's edge, rather than one loop for a window inside the level and one that mirrors every
// read (`ResidualLoop`): V3D runs the one loop faster, Apple's GPUs the two.
override ONE_LOOP: bool = false;

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
// Bound where the source filters: every level, and a capture whose format
// does.
@group(0) @binding(1) var source_sampler: sampler;
@group(1) @binding(0) var<uniform> params: BlurParams;

// One triangle over the whole target; the scissor picks the texels written.
@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

// The texel `i` folds to on a level of `n` texels: past an edge the level is
// mirrored with the edge texel repeated, as `MirrorRepeat` addresses. Two
// integer remainders, which a GPU runs as a long sequence.
fn fold(i: i32, n: i32) -> i32 {
    let period = 2 * n;
    let m = ((i % period) + period) % period;
    return select(m, period - 1 - m, m >= n);
}

fn folded(t: vec2<i32>) -> vec2<i32> {
    return vec2<i32>(fold(t.x, params.level_size.x), fold(t.y, params.level_size.y));
}

// `fold` behind one unsigned compare that passes a texel inside the level.
fn mirror(i: i32, n: i32) -> i32 {
    if (u32(i) < u32(n)) {
        return i;
    }
    return fold(i, n);
}

fn mirrored(t: vec2<i32>) -> vec2<i32> {
    return vec2<i32>(mirror(t.x, params.level_size.x), mirror(t.y, params.level_size.y));
}

fn load(lattice: vec2<i32>) -> vec4<f32> {
    let t = clamp(lattice - params.source_origin, vec2<i32>(0), params.source_max);
    return textureLoad(source, t, 0);
}

fn lattice_texel(position: vec4<f32>) -> vec2<i32> {
    return vec2<i32>(position.xy) + params.target_to_lattice;
}

// A texel of the next level: the source texels at offsets ±½ and ±1½ of its
// center, weighted `[1 3 3 1] / 8` on each axis. Along an axis that is half
// of `[1 3] / 4` and half of `[3 1] / 4`, so a block inside the level is four
// bilinear fetches at quarter phases. A block across the level's edge folds
// every read, where a test per read costs more than it saves.
@fragment
fn fs_tent(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let base = lattice_texel(position) * 2 - vec2<i32>(1);
    if (inside_level(base)) {
        let size = vec2<f32>(textureDimensions(source));
        let first = vec2<f32>(base - params.source_origin);
        let lo = (first + vec2<f32>(1.25)) / size;
        let hi = (first + vec2<f32>(2.75)) / size;
        let sum = textureSampleLevel(source, source_sampler, vec2<f32>(lo.x, lo.y), 0.0)
            + textureSampleLevel(source, source_sampler, vec2<f32>(hi.x, lo.y), 0.0)
            + textureSampleLevel(source, source_sampler, vec2<f32>(lo.x, hi.y), 0.0)
            + textureSampleLevel(source, source_sampler, vec2<f32>(hi.x, hi.y), 0.0);
        return 0.25 * sum;
    }
    return tent_folded(base);
}

// `fs_tent` from a capture whose format does not filter: the block's sixteen
// texels by load.
@fragment
fn fs_tent_loads(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let base = lattice_texel(position) * 2 - vec2<i32>(1);
    if (inside_level(base)) {
        var sum = vec4<f32>(0.0);
        for (var j = 0; j < 4; j++) {
            var row = vec4<f32>(0.0);
            for (var i = 0; i < 4; i++) {
                row += TENT[i] * load(base + vec2<i32>(i, j));
            }
            sum += TENT[j] * row;
        }
        return sum;
    }
    return tent_folded(base);
}

const TENT = array<f32, 4>(0.125, 0.375, 0.375, 0.125);

// Whether the tent's 4 × 4 block from `base` lies inside the level.
fn inside_level(base: vec2<i32>) -> bool {
    return all(base >= vec2<i32>(0)) && all(base + vec2<i32>(3) < params.level_size);
}

fn tent_folded(base: vec2<i32>) -> vec4<f32> {
    var sum = vec4<f32>(0.0);
    for (var j = 0; j < 4; j++) {
        var row = vec4<f32>(0.0);
        for (var i = 0; i < 4; i++) {
            row += TENT[i] * load(folded(base + vec2<i32>(i, j)));
        }
        sum += TENT[j] * row;
    }
    return sum;
}

// The texels `±k` from `at` along `axis`, summed, mirrored past the edge of the level's `n`
// texels where `folds`.
fn pair(at: vec2<i32>, k: i32, axis: vec2<i32>, n: i32, folds: bool) -> vec4<f32> {
    var lo = at - axis * k;
    var hi = at + axis * k;
    if (folds) {
        lo = select(lo, vec2<i32>(mirror(lo.x, n), mirror(lo.y, n)), axis == vec2<i32>(1));
        hi = select(hi, vec2<i32>(mirror(hi.x, n), mirror(hi.y, n)), axis == vec2<i32>(1));
    }
    return load(lo) + load(hi);
}

// The residual along `axis` of a level of `n` texels: the center, then each pair of taps `±k`,
// whose weights match, summed before one multiply, `k` rising. The weights are read four at a
// time, as an array a pass indexes at run time is read through memory.
fn residual(at: vec2<i32>, axis: vec2<i32>, n: i32) -> vec4<f32> {
    let r = params.radius;
    let c = dot(at, axis);
    let folds = c - r < 0 || c + r >= n;
    if (ONE_LOOP) {
        return residual_pairs(at, axis, n, folds);
    }
    // A window inside the level reads straight.
    if (folds) {
        return residual_pairs(at, axis, n, true);
    }
    return residual_pairs(at, axis, n, false);
}

fn residual_pairs(at: vec2<i32>, axis: vec2<i32>, n: i32, folds: bool) -> vec4<f32> {
    let r = params.radius;
    var sum = params.taps[0].x * load(at);
    for (var j = 0; j * 4 <= r; j++) {
        let w = params.taps[j];
        let k = j * 4;
        if (k > 0) {
            sum += w.x * pair(at, k, axis, n, folds);
        }
        if (k + 1 > r) {
            break;
        }
        sum += w.y * pair(at, k + 1, axis, n, folds);
        if (k + 2 > r) {
            break;
        }
        sum += w.z * pair(at, k + 2, axis, n, folds);
        if (k + 3 > r) {
            break;
        }
        sum += w.w * pair(at, k + 3, axis, n, folds);
    }
    return sum;
}

@fragment
fn fs_horizontal(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return residual(mirrored(lattice_texel(position)), vec2<i32>(1, 0), params.level_size.x);
}

@fragment
fn fs_vertical(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return residual(mirrored(lattice_texel(position)), vec2<i32>(0, 1), params.level_size.y);
}
