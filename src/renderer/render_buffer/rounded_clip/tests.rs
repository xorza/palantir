//! What a rounded clip leaves of each draw it cuts, read back from the GPU.

use crate::gpu::pipeline::quad_pipeline::QuadPipeline;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::icons::icon_table::IconTable;
use crate::icons::internals::ONE_COLOR;
use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::icon_handle::IconHandle;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::Image;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::render_buffer::rounded_clip::RoundedClip;
use crate::shape::Shape;
use crate::shape::icon_paint::IconPaint;
use crate::shape::image_paint::ImagePaint;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

/// The surface every case paints on, black where nothing draws.
const SURFACE: UVec2 = UVec2::new(48, 32);

/// The clip: its left edge at 10.25 px and its right edge at 30.25 px, both straight from y = 4
/// down past the rows the cases read, so a pixel there takes the edge's ramp alone.
const CLIP: Rect = Rect::new(10.25, 4.0, 20.0, 24.0);

/// What each family draws inside the clip: an opaque white cover of the whole surface. A text run
/// draws through the raster pipeline's fragment an icon does.
#[derive(Clone, Copy, Debug)]
enum Family {
    Quad,
    Image,
    Mesh,
    Curve,
    Icon,
}

/// What the families draw with: a white image, a white mesh over the surface, and a one-color
/// icon, a rect filling its view box.
#[derive(Debug)]
struct Covers {
    white: ImageHandle,
    mesh: Mesh,
    icon: IconHandle,
}

impl Family {
    const ALL: [Self; 5] = [Self::Quad, Self::Image, Self::Mesh, Self::Curve, Self::Icon];

    fn draw(self, ui: &mut Ui, covers: &Covers) {
        let cover = Rect::new(-12.0, -6.0, SURFACE.x as f32 + 4.0, SURFACE.y as f32 + 4.0);
        match self {
            Self::Quad => {
                ui.add_shape(Shape::rect().at(cover), RgbaF32::WHITE);
            }
            Self::Image => {
                ui.add_shape(
                    Shape::rect().at(cover),
                    ImagePaint::new(covers.white.clone()).fit(Fit::Fill),
                );
            }
            Self::Mesh => {
                ui.add_shape(Shape::mesh(&covers.mesh), RgbaF32::WHITE);
            }
            Self::Icon => {
                // Its 2 : 1 view box over the rows the cases read, (−12, 4, 64 × 32) at scale 1.
                ui.add_shape(
                    Shape::rect().at(Rect::new(-12.0, 4.0, 64.0, 32.0)),
                    IconPaint::new(covers.icon).tint(RgbaF32::WHITE),
                );
            }
            Self::Curve => {
                // A stroke 40 px wide along row 16 covers every row the cases read.
                let points = [
                    Vec2::new(0.0, 22.0),
                    Vec2::new(SURFACE.x as f32 + 12.0, 22.0),
                ];
                ui.add_shape(
                    Shape::polyline(&points).at(cover),
                    Stroke::new(RgbaF32::WHITE, 40.0),
                );
            }
        }
    }
}

/// A harness on the test GPU at scale 1 whose edges stay where layout puts them, so the clip's
/// edge lands a quarter into a pixel.
fn harness() -> Harness {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(SURFACE)
        .mono()
        .depth(Depth::gpu(gpu.handles()))
        .build();
    h.set_pixel_snap(false);
    h.ui_mut().theme_mut().panel_chrome = None;
    h.ui_mut().theme_mut().window_bg = RgbaF32::BLACK;
    h
}

/// The panel at `rect`, rounded by `corners`, that clips what `body` draws.
fn clipped(ui: &mut Ui, salt: &str, rect: Rect, corners: Corners, body: impl FnOnce(&mut Ui)) {
    Panel::canvas()
        .id_salt(salt)
        .at(rect.min)
        .size((Sizing::fixed(rect.size.w), Sizing::fixed(rect.size.h)))
        .chrome(Chrome::rounded(RgbaF32::TRANSPARENT, corners))
        .clip_rounded()
        .show(ui, body);
}

/// `body` on a canvas over the whole surface, which places each panel where it says.
fn on_canvas(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::canvas()
        .id_salt("canvas")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, body);
}

/// White at linear coverage `c` over black, as the target stores it: sRGB bytes.
fn gray(c: f32) -> [u8; 4] {
    let s = RgbaF32::new(c, c, c, 1.0).to_srgba8();
    [s.r, s.g, s.b, s.a]
}

/// Whether `got` is `want` within one step a lane: the GPU's sRGB encode rounds within one step
/// of the CPU's, as `effect::tests` holds its blends.
fn near(got: [u8; 4], want: [u8; 4]) -> bool {
    got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1)
}

/// Every family that draws under a rounded clip takes its coverage. Pixel centers on row 16 lie
/// between the clip's arcs, so the straight edges alone decide them: column 10's center, 10.5, lies
/// 0.25 inside the left edge at 10.25, coverage `(0.5 + 0.25) / 1 = 0.75`; column 30's, 30.5, lies
/// 0.25 past the right edge at 30.25, coverage 0.25; column 20 is whole and column 5 is cut away.
/// Every corner rounds by 4, so the clip is a rounded clip and not a scissor.
#[test]
fn every_family_takes_the_clips_coverage() {
    let mut h = harness();
    let white = h
        .ui()
        .load_image(&Image::from_srgba8(UVec2::splat(2), vec![255; 16]).unwrap())
        .unwrap();
    let icons = h
        .ui()
        .load_icons(IconTable::from_svgs([("rect", ONE_COLOR)]).unwrap());
    let icon = icons.by_name("rect").unwrap();
    let mut mesh = Mesh::with_capacity(4, 6);
    let corner = |x: f32, y: f32| Vec2::new(x, y);
    let a = mesh.vertex(corner(-20.0, -20.0), RgbaF32::WHITE);
    let b = mesh.vertex(corner(60.0, -20.0), RgbaF32::WHITE);
    let c = mesh.vertex(corner(60.0, 40.0), RgbaF32::WHITE);
    let d = mesh.vertex(corner(-20.0, 40.0), RgbaF32::WHITE);
    mesh.triangle(a, b, c);
    mesh.triangle(a, c, d);
    let covers = Covers { white, mesh, icon };
    for family in Family::ALL {
        h.prime(2, |ui| {
            on_canvas(ui, |ui| {
                clipped(ui, "clip", CLIP, Corners::all(4.0), |ui| {
                    family.draw(ui, &covers);
                });
            });
        });
        let image = h.read_back();
        let at = |x: u32| image.get_pixel(x, 16).0;
        for (x, coverage) in [(5, 0.0), (10, 0.75), (20, 1.0), (30, 0.25), (40, 0.0)] {
            assert!(
                near(at(x), gray(coverage)),
                "{family:?} at column {x}: got {:?}, want {:?}",
                at(x),
                gray(coverage)
            );
        }
    }
}

/// Nested clips multiply. The outer clip's left edge lies at 10.25 and the inner's top edge at
/// 6.25, each straight past pixel (10, 6), as the outer rounds only its other corners and the inner
/// starts left of it: its center (10.5, 6.5) lies 0.25 inside both, coverage `0.75 · 0.75 =
/// 0.5625`. Neither clip holds the other, so the chain keeps both.
#[test]
fn nested_clips_multiply() {
    let mut h = harness();
    h.prime(2, |ui| {
        on_canvas(ui, |ui| {
            clipped(
                ui,
                "outer",
                Rect::new(10.25, 2.0, 20.0, 24.0),
                Corners::new(0.0, 4.0, 4.0, 4.0),
                |ui| {
                    clipped(
                        ui,
                        "inner",
                        Rect::new(-2.25, 4.25, 30.0, 20.0),
                        Corners::new(0.0, 4.0, 4.0, 4.0),
                        |ui| {
                            ui.add_shape(
                                Shape::rect().at(Rect::new(-20.0, -20.0, 80.0, 60.0)),
                                RgbaF32::WHITE,
                            );
                        },
                    );
                },
            );
        });
    });
    let image = h.read_back();
    let got = image.get_pixel(10, 6).0;
    assert!(near(got, gray(0.5625)), "got {got:?}");
}

/// The clip's distance, the CPU twin of `palantir_sdf_rounded_rect`, by hand on (0, 0, 20 × 10)
/// of radius 4: the center lies 5 inside, the top edge's middle 0, and (−3, −4) lies
/// `√(7² + 8²) − 4 ≈ 6.63` past the arc about (4, 4).
#[test]
fn a_clips_distance_is_its_rounded_rects() {
    let clip = RoundedClip {
        mask_rect: Rect::new(0.0, 0.0, 20.0, 10.0),
        corners: Corners::all(4.0),
    };
    assert_eq!(clip.distance(Vec2::new(10.0, 5.0)), -5.0);
    assert_eq!(clip.distance(Vec2::new(10.0, 0.0)), 0.0);
    assert_eq!(
        clip.distance(Vec2::new(-3.0, -4.0)),
        Vec2::new(7.0, 8.0).length() - 4.0
    );
}

/// A clip holds a rect whose every point lies the reach and the slack, `0.5 + 1/64 = 33/64`,
/// inside it: on (0, 0, 20 × 10) of radius 4 a point at y = 33/64 below the top edge's middle lies
/// just that deep and is held; one at y = 0.5 is not. A rect into a corner past the arc is not held
/// though its corners on the straight edges are.
#[test]
fn a_clip_holds_what_it_covers_whole() {
    let clip = RoundedClip {
        mask_rect: Rect::new(0.0, 0.0, 20.0, 10.0),
        corners: Corners::all(4.0),
    };
    let point = |y: f32| Rect::new(10.0, y, 0.0, 0.0);
    assert!(clip.holds(point(33.0 / 64.0)));
    assert!(!clip.holds(point(0.5)));
    assert!(clip.holds(Rect::new(4.0, 1.0, 12.0, 8.0)));
    assert!(
        !clip.holds(Rect::new(0.6, 0.6, 10.0, 8.0)),
        "its corner lies past the arc"
    );
}

/// The input of the SDF probe, as WGSL lays it out: a vec4 starts at 32.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SdfIn {
    p: [f32; 2],
    min: [f32; 2],
    size: [f32; 2],
    pad: [f32; 2],
    radius: [f32; 4],
}

const SDF_PROBE: &str = "
struct SdfIn { p: vec2<f32>, min: vec2<f32>, size: vec2<f32>, pad: vec2<f32>, radius: vec4<f32> }
@group(0) @binding(14) var<storage, read> probe_in: array<SdfIn>;
@group(0) @binding(15) var<storage, read_write> probe_out: array<f32>;
@compute @workgroup_size(1)
fn probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let q = probe_in[id.x];
    probe_out[id.x] = palantir_sdf_rounded_rect(q.p - q.min, q.size, q.radius);
}
";

/// The CPU's distance follows the GPU's within 1/128 px, the bound `RoundedClip::SLACK_PX` takes
/// twice over: at the center, on each edge, on each arc and past each corner of a clip of unequal
/// radii, one corner sharp, at a coordinate near 2^12.
#[test]
fn the_cpu_distance_follows_the_shader() {
    let clip = RoundedClip {
        mask_rect: Rect::new(4000.25, 4.0, 20.0, 24.0),
        corners: Corners::new(8.0, 2.0, 0.0, 5.0),
    };
    let min = clip.mask_rect.min;
    let points: Vec<Vec2> = [
        (10.0, 12.0),
        (0.0, 12.0),
        (20.0, 12.0),
        (10.0, 0.0),
        (10.0, 24.0),
        (1.0, 1.0),
        (19.5, 0.5),
        (20.0, 24.0),
        (1.0, 23.0),
        (-3.0, -4.0),
        (23.0, -1.0),
        (22.0, 26.0),
        (-2.0, 27.0),
        (2.34, 2.34),
    ]
    .into_iter()
    .map(|(x, y)| min + Vec2::new(x, y))
    .collect();
    let inputs: Vec<SdfIn> = points
        .iter()
        .map(|&p| SdfIn {
            p: p.to_array(),
            min: min.to_array(),
            size: [clip.mask_rect.size.w, clip.mask_rect.size.h],
            pad: [0.0; 2],
            radius: clip.corners.to_array(),
        })
        .collect();
    let gpu: Vec<f32> = QuadPipeline::SHADER.probe(SDF_PROBE, &inputs);
    for (p, g) in points.iter().zip(gpu) {
        let c = clip.distance(*p);
        assert!((c - g).abs() <= 1.0 / 128.0, "at {p}: cpu {c}, gpu {g}");
    }
}
