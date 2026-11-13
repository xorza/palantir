//! Glass at its rim: the spectrum it spreads, read back in linear `f16` with no dither, and the
//! shader's lens functions probed against their closed forms.

use crate::effect::stock;
use crate::gpu::resource::shader_body::ShaderBody;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::libm_ext::Libm;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::lens::Lens;
use crate::primitives::paint::material::lens_optics::{LINE_D, LensOptics};
use crate::primitives::paint::material::lens_optics::{SPECTRUM_RED, SPECTRUM_VIOLET};
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use glam::UVec2;
use half::f16;

const SIZE: UVec2 = UVec2::new(96, 48);
/// A backdrop whose left bezel spans x 16..32 on its middle row.
const BACKDROP: Rect = Rect::new(16.0, 8.0, 64.0, 32.0);
const ROW: u32 = 24;

/// Strong glass of `refractive_index` with no glint, so a pixel holds what the lens shows and
/// nothing more.
fn lens(refractive_index: f32, dispersion: f32) -> Lens {
    Lens {
        bezel: 16.0,
        thickness: 12.0,
        refractive_index,
        dispersion,
        highlight: 0.0,
    }
}

/// The window's linear pixels, `[r, g, b, a]` a pixel, with `under` painted before a backdrop of
/// `lens`, blurred by σ `blur`.
fn frame(under: &[(Rect, RgbaF32)], lens: Lens, blur: f32) -> Vec<[f32; 4]> {
    let gpu = headless_test_gpu();
    let mut window = Harness::builder(SIZE)
        .depth(Depth::gpu_float(gpu.handles()))
        .build();
    window.ui_mut().theme_mut().window_bg = RgbaF32::BLACK;
    let mut app = |ui: &mut Ui| {
        Panel::canvas()
            .id_salt("dispersion-scene")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for &(rect, color) in under {
                    ui.add_shape(Shape::rect().at(rect), color);
                }
                ui.add_shape(
                    Shape::rect().at(BACKDROP).corners(Corners::all(16.0)),
                    Material::glass(lens).with_backdrop_blur(blur),
                );
            });
    };
    let mut bytes = Vec::new();
    window.capture(&mut app, &mut bytes).unwrap();
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|texel| {
            let lane = |at: usize| f16::from_le_bytes([texel[at], texel[at + 1]]).to_f32();
            [lane(0), lane(2), lane(4), lane(6)]
        })
        .collect()
}

/// The backdrop's left bezel on its middle row, x 16..32.
fn bezel(pixels: &[[f32; 4]]) -> &[[f32; 4]] {
    let start = (ROW * SIZE.x + 16) as usize;
    &pixels[start..start + 16]
}

/// Over a field of one color the spectrum sums back to that color, bit for bit in `f16`, as
/// glass of one index shows it: each XYZ channel's weights sum to 1, so `M⁻¹ · M` alone stands
/// between them. Sharp, where a sample is one bilinear read, and blurred, where it is the
/// B-spline over a coarser lattice. `(0.25, 0.5, 0.75)` sits far from an `f16` rounding tie.
#[test]
fn a_field_of_one_color_keeps_its_color_through_the_spectrum() {
    let field = [(
        Rect::new(0.0, 0.0, 96.0, 48.0),
        RgbaF32::new(0.25, 0.5, 0.75, 1.0),
    )];
    for blur in [0.0, 4.0] {
        let plain = frame(&field, lens(1.5, 0.0), blur);
        let spread = frame(&field, lens(1.5, 0.3), blur);
        assert_eq!(spread, plain, "σ {blur}");
        assert_eq!(bezel(&spread)[4], [0.25, 0.5, 0.75, 1.0], "σ {blur}");
    }
}

/// A white edge under the bezel: glass of one index shows it neutral, and dispersive glass
/// fringes it, warm where white lies outward and cool where it lies inward. Violet bends most,
/// so its sample lands furthest in: a pixel whose inner samples miss the white keeps the red end
/// of the spectrum, and one whose outer samples miss it keeps the violet end. Each fringe is
/// one-sided: a part of the spectrum cut at one end has its hue on one side of white alone, to
/// within 2e-3, two `f16` steps at 1, for each channel's rounding.
#[test]
fn a_white_edge_fringes_warm_outward_and_cool_inward() {
    let white = RgbaF32::WHITE;
    let outward = [(Rect::new(0.0, 0.0, 22.0, 48.0), white)];
    let inward = [(Rect::new(22.0, 0.0, 74.0, 48.0), white)];
    let warmth = |pixels: &[[f32; 4]]| {
        let warm = bezel(pixels).iter().map(|p| p[0] - p[2]);
        let most = warm.clone().fold(f32::MIN, f32::max);
        let least = warm.fold(f32::MAX, f32::min);
        (most, least)
    };
    for under in [&outward, &inward] {
        let plain = frame(under, lens(1.5, 0.0), 0.0);
        assert!(
            bezel(&plain).iter().all(|p| p[0] == p[1] && p[1] == p[2]),
            "one index keeps white neutral: {:?}",
            bezel(&plain)
        );
    }
    let (most, least) = warmth(&frame(&outward, lens(1.5, 0.3), 0.0));
    assert!(
        most > 0.05 && least > -2e-3,
        "white outward: {most}, {least}"
    );
    let (most, least) = warmth(&frame(&inward, lens(1.5, 0.3), 0.0));
    assert!(
        least < -0.05 && most < 2e-3,
        "white inward: {most}, {least}"
    );
}

/// The stock glass's WGSL over the backdrop contract, which its functions run on as they draw.
fn glass_shader() -> ShaderBody {
    ShaderBody {
        label: "palantir.effect.glass",
        wgsl: Box::leak(stock::glass_wgsl().into_boxed_str()),
        includes: &[
            include_str!("../../../effect/contract.wgsl"),
            include_str!("../../../effect/backdrop.wgsl"),
        ],
        constants: &[],
    }
}

const LENS_INDEX_PROBE: &str = "
@group(0) @binding(14) var<storage, read> probe_in: array<vec4<f32>>;
@group(0) @binding(15) var<storage, read_write> probe_out: array<f32>;
@compute @workgroup_size(1)
fn probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let q = probe_in[id.x];
    probe_out[id.x] = glass_index(vec4<f32>(16.0, 12.0, q.x, q.y), q.z);
}
";

/// The shader's index along the spectrum, `(index, dispersion, t)` in: at the violet end it is
/// [`LensOptics::violet_index`], which the reach is taken at; at the D line it is the lens's
/// index; at the red end it is `n − 0.4390 · dispersion` (Cauchy's offset at 700 nm). Where that
/// falls below 1, at index 1 and dispersion 1, it holds at 1. Within 8 ULP, through the
/// reciprocals of squares and a `mix`.
#[test]
fn the_index_runs_along_the_spectrum_and_never_below_one() {
    let u = |wavelength: f32| 1.0 / (wavelength * wavelength);
    let at_d = (u(LINE_D) - u(SPECTRUM_RED)) / (u(SPECTRUM_VIOLET) - u(SPECTRUM_RED));
    let cases = [
        ([1.5, 0.06, 1.0, 0.0], LensOptics::violet_index(1.5, 0.06)),
        ([1.5, 0.06, at_d, 0.0], 1.5),
        ([1.5, 0.06, 0.0, 0.0], 1.5 - 0.06 * 0.439_047),
        ([1.0, 1.0, 0.0, 0.0], 1.0),
        ([1.0, 1.0, 1.0, 0.0], LensOptics::violet_index(1.0, 1.0)),
    ];
    let inputs: Vec<[f32; 4]> = cases.iter().map(|(input, _)| *input).collect();
    let got: Vec<f32> = glass_shader().probe(LENS_INDEX_PROBE, &inputs);
    for ((input, want), got) in cases.iter().zip(got) {
        assert!(
            (got - want).abs() <= 8.0 * f32::EPSILON * want,
            "{input:?}: {got} against {want}"
        );
    }
}

const LENS_GLINT_PROBE: &str = "
@group(0) @binding(14) var<storage, read> probe_in: array<vec4<f32>>;
@group(0) @binding(15) var<storage, read_write> probe_out: array<f32>;
@compute @workgroup_size(1)
fn probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let q = probe_in[id.x];
    probe_out[id.x] = glass_glint(vec4<f32>(16.0, 12.0, 1.5, 0.0), q.x, q.yz);
}
";

/// The shader's glint, `(inward, outward)` in, on a bezel 16 px wide and 12 px high at index
/// 1.5, against the lamp's own closed form in `f64`: Schlick's Fresnel at the surface's tilt
/// `θ`, `f0 = ((n − 1) / (n + 1))² = 0.04`, times `sin θ · max(outward · lamp, 0)`, the 3D
/// normal against a lamp low at the top left. It reaches 0 where the bezel meets the flat top,
/// which takes no glint, so the backdrop shows no step there, and holds 0 on a bezel facing away.
/// The edge itself, where the slope is vertical, takes the whole facing. Within 1e-6, through
/// the shader's `pow`.
#[test]
fn the_glint_fades_to_nothing_where_the_bezel_meets_the_flat_top() {
    let (bezel, thickness, f0) = (16.0_f64, 12.0_f64, 0.04_f64);
    let glint = |inward: f64, facing: f64| {
        let g = 1.0 - (inward / bezel).clamp(0.0, 1.0);
        let rise = (1.0 - g.libm_powi(4)).max(1e-30);
        let slope = thickness / bezel * g.libm_powi(3) * rise.libm_powf(-0.75);
        let cos = 1.0 / slope.libm_hypot(1.0);
        let sin = slope / slope.libm_hypot(1.0);
        (f0 + (1.0 - f0) * (1.0 - cos).libm_powi(5)) * sin * facing.max(0.0)
    };
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    let (left, top_left, right) = ([-1.0, 0.0], [-diagonal, -diagonal], [1.0, 0.0]);
    let facing_left = f64::from(diagonal);
    let cases = [
        (0.0, left, glint(0.0, facing_left)),
        (8.0, left, glint(8.0, facing_left)),
        (8.0, top_left, glint(8.0, 1.0)),
        (15.9, left, glint(15.9, facing_left)),
        (16.0, left, 0.0),
        (4.0, right, 0.0),
    ];
    let inputs: Vec<[f32; 4]> = cases
        .iter()
        .map(|&(inward, [x, y], _)| [inward, x, y, 0.0])
        .collect();
    let got: Vec<f32> = glass_shader().probe(LENS_GLINT_PROBE, &inputs);
    for (&(inward, outward, want), got) in cases.iter().zip(got) {
        assert!(
            (f64::from(got) - want).abs() <= 1e-6,
            "inward {inward}, outward {outward:?}: {got} against {want}"
        );
    }
}
