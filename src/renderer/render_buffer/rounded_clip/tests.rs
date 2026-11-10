//! What a rounded clip leaves of each draw it cuts, read back from the GPU.

use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::icons::icon_set::IconSet;
use crate::icons::icon_table::IconTable;
use crate::icons::internals::ONE_COLOR;
use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::icon_handle::IconHandle;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::{Image, ImageFit};
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::shape::icon_paint::IconPaint;
use crate::shape::image_paint::ImagePaint;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};
use std::rc::Rc;

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
                    ImagePaint::new(covers.white.clone()).fit(ImageFit::Fill),
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
    let icons: IconSet = h.ui().load_icons(Rc::new(
        IconTable::from_svgs([("rect", ONE_COLOR)]).unwrap(),
    ));
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
