//! Every draw kind at a rect clip's edge: flush inside it, which no scissor need cut, and across
//! it, which the clip cuts.

use palantir::glam::{UVec2, Vec2};
use palantir::widget::{
    CubicBezier, IconPaint, ImagePaint, Mesh, PaintAnimation, PaintedBy, RectShape, Shape,
};
use palantir::{
    Block, Chrome, Configure, Corners, Fit, Image, Paint, Panel, Rect, RgbaF32, Shadow, Sizing,
    Stroke, Text, Ui,
};

use crate::fixtures::{self, icon};
use crate::golden_name::GoldenName;
use crate::visual_rig::VisualRig;

const FILL: RgbaF32 = RgbaF32::srgb(0.42, 0.55, 0.78);
const INK: RgbaF32 = RgbaF32::srgb(0.95, 0.65, 0.25);

/// `paint` over a cell at `at`, `id` apart.
fn cell<P>(ui: &mut Ui, id: &str, at: Rect, paint: P)
where
    RectShape: PaintedBy<P>,
{
    Panel::zstack()
        .id_salt(id)
        .at(at.min)
        .size((Sizing::fixed(at.size.w), Sizing::fixed(at.size.h)))
        .show(ui, |ui| {
            ui.add_shape(Shape::rect(), paint);
        });
}

/// Under a rect clip of (20, 20, 200 × 140), each kind flush inside the clip's left edge and across
/// its right: a rect, a text run, an image and an icon; a curve across the left edge and one
/// inside; a shadowed block across the bottom; a mesh across the left; a rect turned 30° across the
/// top. Each draws only inside the clip, a cut draw to the clip's edge.
#[test]
fn every_draw_kind_at_a_clip_edge_matches_golden() {
    let mut rig = VisualRig::new();
    let image = Image::from_srgba8(
        UVec2::new(2, 1),
        [[230, 60, 60, 255], [60, 120, 230, 255]].concat(),
    )
    .unwrap();
    let handle = rig.host.ui().load_image(&image).unwrap();
    let mut mesh = Mesh::new();
    let a = mesh.add_vertex(Vec2::new(-20.0, 110.0), RgbaF32::srgb(1.0, 0.3, 0.3));
    let b = mesh.add_vertex(Vec2::new(30.0, 125.0), RgbaF32::srgb(0.3, 1.0, 0.4));
    let c = mesh.add_vertex(Vec2::new(-20.0, 140.0), RgbaF32::srgb(0.3, 0.5, 1.0));
    mesh.add_triangle(a, b, c);
    let img = rig
        .size(UVec2::new(240, 180))
        .frame(|ui| {
            let icons = ui.load_icons(icon::atlas());
            let solid =
                IconPaint::new(icons.by_name("solid").expect("fixture icon")).fit(Fit::Fill);
            let picture = ImagePaint::new(handle.clone()).fit(Fit::Fill);
            fixtures::canvas(ui, |ui| {
                Panel::canvas()
                    .id_salt("clip")
                    .at((20.0, 20.0))
                    .size((Sizing::fixed(200.0), Sizing::fixed(140.0)))
                    .clip_rect()
                    .show(ui, |ui| {
                        cell(ui, "rect-in", Rect::new(0.0, 0.0, 40.0, 14.0), FILL);
                        cell(ui, "rect-across", Rect::new(180.0, 0.0, 40.0, 14.0), FILL);
                        Text::new("Edge")
                            .id_salt("text-in")
                            .at((0.0, 18.0))
                            .show(ui);
                        Text::new("Across the edge")
                            .id_salt("text-across")
                            .at((150.0, 18.0))
                            .show(ui);
                        cell(
                            ui,
                            "image-in",
                            Rect::new(0.0, 42.0, 40.0, 20.0),
                            picture.clone(),
                        );
                        cell(
                            ui,
                            "image-across",
                            Rect::new(180.0, 42.0, 40.0, 20.0),
                            picture,
                        );
                        cell(ui, "icon-in", Rect::new(0.0, 66.0, 20.0, 20.0), solid);
                        cell(ui, "icon-across", Rect::new(190.0, 66.0, 20.0, 20.0), solid);
                        for (from, to) in [(-20.0, 40.0), (60.0, 100.0)] {
                            ui.add_shape(
                                Shape::cubic_bezier(CubicBezier {
                                    p0: Vec2::new(from, 96.0),
                                    p1: Vec2::new(from + 15.0, 86.0),
                                    p2: Vec2::new(to - 15.0, 106.0),
                                    p3: Vec2::new(to, 96.0),
                                }),
                                Stroke::new(INK, 3.0),
                            );
                        }
                        Block::new()
                            .id_salt("shadowed")
                            .at((60.0, 118.0))
                            .size((60.0, 40.0))
                            .chrome(Chrome {
                                corners: Corners::all(6.0),
                                paint: Paint {
                                    shadow: Shadow::drop(RgbaF32::BLACK, Vec2::new(0.0, 2.0), 4.0),
                                    ..Paint::from(FILL)
                                },
                            })
                            .show(ui);
                        ui.add_shape(Shape::mesh(&mesh), RgbaF32::WHITE);
                        if let Some(shape) =
                            ui.add_shape(Shape::rect().at(Rect::new(110.0, -8.0, 40.0, 20.0)), INK)
                        {
                            shape.animate(PaintAnimation::turn(1.0 / 12.0, 1.0 / 12.0));
                        }
                    });
            });
        })
        .image;
    GoldenName::ClipEdgeDraws.assert_matches(&img);
}
