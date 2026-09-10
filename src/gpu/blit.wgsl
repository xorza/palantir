// Presents the retained backbuffer onto a target that cannot be copied into.
//
// A GLES swapchain image *is* the default framebuffer: nothing can be copied
// onto it, so EGL offers `RENDER_ATTACHMENT` alone and
// `copy_texture_to_texture` is unavailable. Drawing the backbuffer as a
// full-screen quad reaches the same pixels through the one usage every surface
// has, which is what keeps damage-limited painting working there instead of
// repainting the whole window.
//
// One triangle rather than two: a single oversized triangle covers the
// viewport with no seam down the diagonal, and its corners come from the
// vertex index, so there is no vertex buffer to bind or keep.

@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // (0,0), (2,0), (0,2) — which land at (-1,-1), (3,-1), (-1,3) in clip
    // space, a triangle that contains the whole visible square.
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

@group(0) @binding(0) var tex: texture_2d<f32>;

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    // `textureLoad` rather than `textureSample`: this stands in for a texture
    // copy, so it has to read one texel and not a weighted pair of them. The
    // group-0 sampler the layout carries is a *linear* one — shared with the
    // image draws, which snap their own UVs — and sampling through it put a
    // fraction of each neighbour into every pixel next to an edge. The visual
    // goldens caught it as dark background lifting from 16 to 26.
    //
    // The fragment's own position is the texel index, and source and
    // destination are the same size, so there is no coordinate to get wrong.
    return textureLoad(tex, vec2<i32>(pos.xy), 0);
}
