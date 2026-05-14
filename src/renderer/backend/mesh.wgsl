struct Viewport { size: vec2<f32> };
@group(0) @binding(0) var<uniform> vp: Viewport;

struct VsIn {
    @location(0) pos: vec2<f32>,
    // Linear-u8 lanes — `Unorm8x4` auto-normalizes `u8/255` to
    // `0..1` floats with no decode. Stored linearly on the CPU
    // (`From<Color> for ColorU8` is a linear quantize), so the
    // rasterizer interpolates linear values directly.
    @location(1) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs(in: VsIn) -> VsOut {
    let ndc = vec2<f32>(
        in.pos.x / vp.size.x * 2.0 - 1.0,
        1.0 - in.pos.y / vp.size.y * 2.0,
    );
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
