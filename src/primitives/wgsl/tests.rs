//! The shared WGSL, run on the GPU through the prelude every pipeline takes.

use crate::gpu::pipeline::shape_template;
use crate::gpu::resource::shader_body::ShaderBody;
use crate::primitives::wgsl::SHAPE_TEMPLATE;

const COMPOSITE_PROBE: &str = "
@group(0) @binding(14) var<storage, read> probe_in: array<array<vec4<f32>, 3>>;
@group(0) @binding(15) var<storage, read_write> probe_out: array<vec4<f32>>;
@compute @workgroup_size(1)
fn probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let q = probe_in[id.x];
    probe_out[id.x] = palantir_composite(q[0].x, q[1], q[2], q[0].y);
}
";

/// A box's fill and border weigh by area, `(o − i) · band + i · F`, on the GPU, each value
/// hand-computed. The antialiasing half-width is 0.5, so an edge at `d` covers `0.5 − d`, clamped:
/// at `d = 0` and a border 0.5 wide, `o = 0.5` and `i = 0`; at `d = −0.25`, `o = 0.75` and
/// `i = 0.25`. With the fill `F = (0.2, 0.4, 0.6) · 0.5`, premultiplied `(0.1, 0.2, 0.3, 0.5)`, and
/// the border `B = (1, 0, 0) · 0.5`, the band shows `B + F · (1 − 0.5) = (0.55, 0.1, 0.15, 0.75)`:
/// `0.5 · band` at `d = 0`, `0.5 · band + 0.25 · F` at `d = −0.25`.
///
/// An opaque blue fill under an opaque white border reads its coverage: 0.5 at `d = 0`, and 0.75
/// at `d = −0.25`, `0.5` of border and `0.25` of fill. A translucent border shows the fill in its
/// band: the first case, not the border alone, `0.5 · B = (0.25, 0, 0, 0.25)`, with what is behind
/// showing through. With no border the fill takes `o`; 2 px inside the box shows its fill, and
/// 2 px out nothing. Within 1e-6, the rounding of the f32 products.
#[test]
fn a_boxs_fill_and_border_weigh_by_area() {
    let fill = [0.2, 0.4, 0.6, 0.5];
    let border = [1.0, 0.0, 0.0, 0.5];
    let (blue, white) = ([0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0, 1.0]);
    // `(d, border width)`, the fill, the border, and the premultiplied result.
    let cases = [
        ([0.0, 0.5], fill, border, [0.275, 0.05, 0.075, 0.375]),
        ([-0.25, 0.5], fill, border, [0.3, 0.1, 0.15, 0.5]),
        ([0.0, 0.5], blue, white, [0.5, 0.5, 0.5, 0.5]),
        ([-0.25, 0.5], blue, white, [0.5, 0.5, 0.75, 0.75]),
        ([0.0, 0.0], fill, border, [0.05, 0.1, 0.15, 0.25]),
        ([-2.0, 0.5], fill, border, [0.1, 0.2, 0.3, 0.5]),
        ([2.0, 0.5], fill, border, [0.0; 4]),
        ([-2.0, 0.0], fill, border, [0.1, 0.2, 0.3, 0.5]),
    ];
    let shader = ShaderBody {
        label: "palantir.composite",
        wgsl: "",
        includes: &[],
        constants: &[],
    };
    let inputs: Vec<[[f32; 4]; 3]> = cases
        .iter()
        .map(|&([d, width], fill, border, _)| [[d, width, 0.0, 0.0], fill, border])
        .collect();
    let got: Vec<[f32; 4]> = shader.probe(COMPOSITE_PROBE, &inputs);
    for ((at, fill, border, want), got) in cases.iter().zip(got) {
        assert!(
            got.iter().zip(want).all(|(g, w)| (g - w).abs() <= 1e-6),
            "d {}, border width {}, fill {fill:?}, border {border:?}: {got:?} against {want:?}",
            at[0],
            at[1],
        );
    }
}

const TEMPLATE_PROBE: &str = "
@group(0) @binding(14) var<storage, read> probe_in: array<array<vec4<f32>, 3>>;
@group(0) @binding(15) var<storage, read_write> probe_out: array<array<vec4<f32>, 2>>;
@compute @workgroup_size(1)
fn probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let q = probe_in[id.x];
    let region = u32(q[0].x);
    probe_out[id.x] = array<vec4<f32>, 2>(
        palantir_template_fill(region, q[0].yz, q[0].w, q[1], q[2], 0.5),
        vec4<f32>(palantir_template_coverage(region, q[0].yz, q[0].w)),
    );
}
";

/// A template fragment's fill and coverage read its region, on the GPU, each value hand-computed
/// from the coverage `0.5 − d`, clamped, at the distance `d` its region reads: none in the interior,
/// a band's `geo.x`, a ring's `|geo| − ring_r`. The interior takes the premultiplied fill
/// `(0.1, 0.2, 0.3, 0.5)` and full coverage at a `geo` a band would read as 5 px out, nothing. A
/// band reads `geo.x` alone: at `(0, 9)` the box at `d = 0`, as
/// [`a_boxs_fill_and_border_weigh_by_area`] has it, `(0.275, 0.05, 0.075, 0.375)` and coverage
/// 0.5, and at `(−0.25, 0)` `(0.3, 0.1, 0.15, 0.5)` and 0.75. A ring at `(3, 4)` of radius 5 is at
/// `d = 0` too, where a band would read 3 px out; of radius 4.75 at `d = 0.25`: `o = 0.25`,
/// `i = 0`, the band's `(0.55, 0.1, 0.15, 0.75) · 0.25 = (0.1375, 0.025, 0.0375, 0.1875)` and
/// coverage 0.25. Within 1e-6, the rounding of the f32 products.
#[test]
fn a_template_fragment_reads_its_region() {
    const INTERIOR: f32 = 0.0;
    const BAND: f32 = 1.0;
    const RING: f32 = 2.0;
    let fill = [0.2, 0.4, 0.6, 0.5];
    let border = [1.0, 0.0, 0.0, 0.5];
    let at_edge = [0.275, 0.05, 0.075, 0.375];
    // `(region, geo, ring_r)`, then the fill and the coverage.
    let cases = [
        ([INTERIOR, 5.0, 0.0, 0.0], [0.1, 0.2, 0.3, 0.5], 1.0),
        ([BAND, 0.0, 9.0, 0.0], at_edge, 0.5),
        ([BAND, -0.25, 0.0, 0.0], [0.3, 0.1, 0.15, 0.5], 0.75),
        ([RING, 3.0, 4.0, 5.0], at_edge, 0.5),
        (
            [RING, 3.0, 4.0, 4.75],
            [0.1375, 0.025, 0.0375, 0.1875],
            0.25,
        ),
    ];
    let shader = ShaderBody {
        label: "palantir.template",
        wgsl: "",
        includes: &[SHAPE_TEMPLATE],
        constants: &[shape_template::CONSTANT],
    };
    let inputs: Vec<[[f32; 4]; 3]> = cases.iter().map(|&(at, ..)| [at, fill, border]).collect();
    let got: Vec<[[f32; 4]; 2]> = shader.probe(TEMPLATE_PROBE, &inputs);
    for ((at, want_fill, want_coverage), [got_fill, got_coverage]) in cases.iter().zip(got) {
        assert!(
            got_fill
                .iter()
                .zip(want_fill)
                .all(|(g, w)| (g - w).abs() <= 1e-6),
            "{at:?}: fill {got_fill:?} against {want_fill:?}",
        );
        assert!(
            (got_coverage[0] - want_coverage).abs() <= 1e-6,
            "{at:?}: coverage {} against {want_coverage}",
            got_coverage[0],
        );
    }
}
