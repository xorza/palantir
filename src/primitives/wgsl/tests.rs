//! The shared WGSL, run on the GPU through the prelude every pipeline takes.

use crate::gpu::pipeline::shader_body::ShaderBody;

const COMPOSITE_PROBE: &str = "
@group(0) @binding(14) var<storage, read> probe_in: array<array<vec4<f32>, 3>>;
@group(0) @binding(15) var<storage, read_write> probe_out: array<vec4<f32>>;
@compute @workgroup_size(1)
fn probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let q = probe_in[id.x];
    if (q[0].z == 0.0) {
        probe_out[id.x] = palantir_composite(q[0].x, q[1], q[2], q[0].y);
    } else {
        probe_out[id.x] = palantir_composite_outside(q[0].x, q[1], q[2], q[0].y);
    }
}
";

/// A box's fill and border weigh by area, on the GPU, each value hand-computed. The
/// antialiasing half-width is 0.5, so an edge at `d` covers `0.5 − d`, clamped: at `d = 0` and a
/// border 0.5 wide, `o = 0.5` and `i = 0`; at `d = −0.25`, `o = 0.75` and `i = 0.25`. With the fill
/// `F = (0.2, 0.4, 0.6) · 0.5`, premultiplied `(0.1, 0.2, 0.3, 0.5)`, and the border
/// `B = (1, 0, 0) · 0.5`, the band shows `B + F · (1 − 0.5) = (0.55, 0.1, 0.15, 0.75)`:
/// - inside, `(o − i) · band + i · F`: `0.5 · band` at `d = 0`, `0.5 · band + 0.25 · F` at
///   `d = −0.25`;
/// - outside, `(o − i) · band + (1 − o) · F`: `0.5 · band + 0.5 · F` at `d = 0`,
///   `0.5 · band + 0.25 · F` at `d = −0.25`.
///
/// An opaque blue fill under an opaque white border reads its coverage: 0.5 inside at `d = 0`,
/// where a material's fill at `o` under the border read 0.75, and 0.75 outside at `d = −0.25`,
/// `0.5` of border and `0.25` of fill, where it read 0.875. A translucent border shows the fill
/// in its band: the first case, where a quad's fill inside the border read the border alone,
/// `0.5 · B = (0.25, 0, 0, 0.25)`, and showed what is behind. With no border the fill takes `o` inside and
/// `1 − o` outside; 2 px inside, the box shows its fill and the outside nothing, and 2 px out the
/// reverse. Within 1e-6, the rounding of the f32 products.
#[test]
fn a_boxs_fill_and_border_weigh_by_area() {
    const INSIDE: f32 = 0.0;
    const OUTSIDE: f32 = 1.0;
    let fill = [0.2, 0.4, 0.6, 0.5];
    let border = [1.0, 0.0, 0.0, 0.5];
    let (blue, white) = ([0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0, 1.0]);
    let cases = [
        (
            [0.0, 0.5, INSIDE, 0.0],
            fill,
            border,
            [0.275, 0.05, 0.075, 0.375],
        ),
        (
            [-0.25, 0.5, INSIDE, 0.0],
            fill,
            border,
            [0.3, 0.1, 0.15, 0.5],
        ),
        (
            [0.0, 0.5, OUTSIDE, 0.0],
            fill,
            border,
            [0.325, 0.15, 0.225, 0.625],
        ),
        (
            [-0.25, 0.5, OUTSIDE, 0.0],
            fill,
            border,
            [0.3, 0.1, 0.15, 0.5],
        ),
        ([0.0, 0.5, INSIDE, 0.0], blue, white, [0.5, 0.5, 0.5, 0.5]),
        (
            [-0.25, 0.5, OUTSIDE, 0.0],
            blue,
            white,
            [0.5, 0.5, 0.75, 0.75],
        ),
        (
            [0.0, 0.0, INSIDE, 0.0],
            fill,
            border,
            [0.05, 0.1, 0.15, 0.25],
        ),
        (
            [-0.25, 0.0, OUTSIDE, 0.0],
            fill,
            border,
            [0.025, 0.05, 0.075, 0.125],
        ),
        ([-2.0, 0.5, INSIDE, 0.0], fill, border, [0.1, 0.2, 0.3, 0.5]),
        ([-2.0, 0.5, OUTSIDE, 0.0], fill, border, [0.0; 4]),
        ([2.0, 0.5, INSIDE, 0.0], fill, border, [0.0; 4]),
        ([2.0, 0.5, OUTSIDE, 0.0], fill, border, [0.1, 0.2, 0.3, 0.5]),
        ([-2.0, 0.0, INSIDE, 0.0], fill, border, [0.1, 0.2, 0.3, 0.5]),
        ([2.0, 0.0, OUTSIDE, 0.0], fill, border, [0.1, 0.2, 0.3, 0.5]),
    ];
    let shader = ShaderBody {
        label: "palantir.composite",
        wgsl: "",
        includes: &[],
        constants: &[],
    };
    let inputs: Vec<[[f32; 4]; 3]> = cases
        .iter()
        .map(|&(at, fill, border, _)| [at, fill, border])
        .collect();
    let got: Vec<[f32; 4]> = shader.probe(COMPOSITE_PROBE, &inputs);
    for ((at, fill, border, want), got) in cases.iter().zip(got) {
        assert!(
            got.iter().zip(want).all(|(g, w)| (g - w).abs() <= 1e-6),
            "d {}, border width {}, side {}, fill {fill:?}, border {border:?}: {got:?} against \
             {want:?}",
            at[0],
            at[1],
            at[2],
        );
    }
}
