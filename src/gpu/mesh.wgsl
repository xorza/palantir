struct VsIn {
    @location(0) pos: vec2<f32>,
    // `color` is sRGB-encoded bytes that the `Unorm8x4` fetch normalizes
    // to `0..1`, decoded below per vertex with the exact transfer function
    // rather than a fit, so the rasterizer interpolates linear light.
    // `tint` is linear f16 already.
    //
    // **Straight alpha in, premultiplied alpha out.** `color` and
    // `tint` carry straight-alpha values; `vs` premultiplies their
    // product, so the rasterizer interpolates premultiplied colour across
    // the face — white to transparent black is half-white at half alpha
    // midway, not a dark fringe — and `fs` writes it as is, matching the
    // pipeline's `PREMULTIPLIED_ALPHA_BLENDING` blend state. See AGENTS.md
    // "Colour pipeline" for the shared contract.
    @location(1) color: vec4<f32>,
    // Per-instance transform + tint. `physical = pos * scale + translate`;
    // `out_color = premultiply(color * tint)`.
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
    var out: VsOut;
    out.clip = clip_from_px(in.pos * in.scale + in.translate);
    let straight = vec4<f32>(srgb_to_linear(in.color.rgb), in.color.a) * in.tint;
    out.color = premultiply(straight.rgb, straight.a);
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
