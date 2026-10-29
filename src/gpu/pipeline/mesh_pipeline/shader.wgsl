struct VsIn {
    @location(0) pos: vec2<f32>,
    // `color` is sRGB-encoded bytes that the `Unorm8x4` fetch normalizes
    // to `0..1`, decoded below per vertex with the exact transfer function
    // rather than a fit, so the rasterizer interpolates linear light.
    // `tint` is linear f16 already.
    //
    // **Straight alpha in, premultiplied alpha out.** `color` and
    // `tint` carry straight-alpha values; `vs` premultiplies their
    // product, so the rasterizer interpolates premultiplied color across
    // the face — white to transparent black is half-white at half alpha
    // midway, not a dark fringe — and `fs` writes it as is, matching the
    // pipeline's `PREMULTIPLIED_ALPHA_BLENDING` blend state, the contract
    // `premultiply` in `prelude.wgsl` states.
    @location(1) color: vec4<f32>,
    // Per-instance transform + tint. `physical = rotor_scale ⊗ pos + translate`,
    // a complex product that turns and scales at once;
    // `out_color = premultiply(color * tint)`.
    @location(2) translate: vec2<f32>,
    @location(3) rotor_scale: vec2<f32>,
    @location(4) tint: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs(in: VsIn) -> VsOut {
    var out: VsOut;
    let rs = in.rotor_scale;
    let turned = vec2<f32>(rs.x * in.pos.x - rs.y * in.pos.y, rs.y * in.pos.x + rs.x * in.pos.y);
    out.clip = clip_from_px(turned + in.translate);
    let straight = vec4<f32>(srgb_to_linear(in.color.rgb), in.color.a) * in.tint;
    out.color = premultiply(straight.rgb, straight.a);
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
