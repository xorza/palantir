// Every pipeline's device has dual-source blending (`DeviceRequirements::FEATURES`), which a
// backdrop's effect stage writes through; an enable stands before every declaration.
enable dual_source_blending;

// Shared WGSL prelude. `ShaderBody::specialize` concatenates it
// ahead of every shader in this backend, so everything here has to
// compile in front of every one of them — nothing may declare a
// binding, which is the one thing they disagree about.

// Coverage, at a pixel centered `r` from a band's center line, of the band
// reaching `core_half` to either side: the edge ramp, capped at the share
// of the filter the band's whole width fills, since a band narrower than
// the filter can never cover more of a pixel than its own width.
fn band_coverage(core_half: f32, r: f32) -> f32 {
    let filter_width = 2.0 * PALANTIR_AA_HALF_WIDTH;
    let plateau = clamp(2.0 * core_half / filter_width, 0.0, 1.0);
    return clamp((core_half + PALANTIR_AA_HALF_WIDTH - r) / filter_width, 0.0, plateau);
}

// Unit-quad corners in triangle-strip order, indexed by `vertex_index`.
const CORNERS = array<vec2<f32>, 4>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 1.0),
);

// Straight alpha in, premultiplied out — the contract every fragment
// entry point in this backend writes under, because the blend state is
// `PREMULTIPLIED_ALPHA_BLENDING`.
fn premultiply(rgb: vec3<f32>, alpha: f32) -> vec4<f32> {
    return vec4<f32>(rgb * alpha, alpha);
}
