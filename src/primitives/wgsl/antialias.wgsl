// Antialiased coverage of an edge, which every palantir shader takes from a signed distance.
// The includer declares `PALANTIR_AA_HALF_WIDTH`.

// Coverage, at a pixel centered `d` outside an edge (negative inside), of
// the side the edge bounds: zero from `PALANTIR_AA_HALF_WIDTH` out, full from
// `PALANTIR_AA_HALF_WIDTH` in.
fn palantir_edge_coverage(d: f32) -> f32 {
    return clamp((PALANTIR_AA_HALF_WIDTH - d) / (2.0 * PALANTIR_AA_HALF_WIDTH), 0.0, 1.0);
}
