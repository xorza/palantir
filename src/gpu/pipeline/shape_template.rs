//! What a pipeline that draws the shape template ([`wgsl::SHAPE_TEMPLATE`]) needs beside its
//! text: the constant it substitutes and the vertices it draws.

use crate::gpu::pipeline::shader_body::ShaderConstant;
use crate::primitives::wgsl;

/// The number [`wgsl::SHAPE_TEMPLATE`] takes, which each includer lists among its constants.
pub(crate) const CONSTANT: ShaderConstant =
    ShaderConstant::uint("ARC_SEGMENTS", wgsl::ARC_SEGMENTS);

/// Vertices of a template of `corners` corners: each corner's ring, each edge's band, and the
/// interior fan over every corner's inner chain (`palantir_template_vertex`).
pub(crate) const fn vertices(corners: u32) -> u32 {
    let rings = corners * (2 * wgsl::ARC_SEGMENTS + 1);
    let bands = corners * 4;
    let interior = corners * (wgsl::ARC_SEGMENTS + 1) - 2;
    3 * (rings + bands + interior)
}
