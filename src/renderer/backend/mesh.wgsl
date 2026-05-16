struct Viewport { size: vec2<f32> };
@group(0) @binding(0) var<uniform> vp: Viewport;

struct VsIn {
    @location(0) pos: vec2<f32>,
    // Linear-u8 lanes — `Unorm8x4` auto-normalizes `u8/255` to
    // `0..1` floats with no decode. Stored linearly on the CPU
    // (`From<Color> for ColorU8` is a linear quantize), so the
    // rasterizer interpolates linear values directly.
    //
    // **Straight alpha in, premultiplied alpha out.** `color` and
    // `tint` carry straight-alpha values; `fs` premultiplies at
    // output to match the pipeline's `PREMULTIPLIED_ALPHA_BLENDING`
    // blend state. See the shared shader contract in
    // `docs/review-wgsl-shaders.md` A2 and CLAUDE.md "Colour pipeline".
    @location(1) color: vec4<f32>,
    // Per-instance transform + tint. `physical = pos * scale + translate`;
    // `out_color = color * tint` (straight × straight, premultiplied at fs).
    @location(2) translate: vec2<f32>,
    @location(3) scale: f32,
    @location(4) tint: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs(in: VsIn) -> VsOut {
    let phys = in.pos * in.scale + in.translate;
    let ndc = vec2<f32>(
        phys.x / vp.size.x * 2.0 - 1.0,
        1.0 - phys.y / vp.size.y * 2.0,
    );
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.color = in.color * in.tint;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // `in.color` is straight-alpha linear (vertex.color * tint, both
    // straight). Premultiply at output so the blend pipeline's
    // `PREMULTIPLIED_ALPHA_BLENDING` sees `rgb * a`. Without this,
    // translucent meshes paint too bright (see review A1).
    return vec4<f32>(in.color.rgb * in.color.a, in.color.a);
}
