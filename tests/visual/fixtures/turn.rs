//! A turn paint animation on each shape kind that turns, held at 30°.

use glam::{UVec2, Vec2};
use palantir::widget::LineCap;
use palantir::widget::{GlyphFont, IconFit, Mesh, PaintAnimation, Shape};
use palantir::{
    Configure, Corners, Image, ImageFit, LinearGradient, Panel, Rect, RgbaF32, Shadow, Sizing,
    Stroke, Ui, WidgetId,
};

use std::rc::Rc;

use crate::fixtures::icon;
use crate::golden_name::GoldenName;
use crate::visual_rig::VisualRig;

/// A twelfth of a turn, 30°, from the first frame on.
fn thirty_degrees() -> PaintAnimation {
    PaintAnimation::turn(1.0 / 12.0, 1.0 / 12.0)
}

/// A 100×100 owner whose center (50, 50) is the pivot, with `body` recorded inside it.
fn owner(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui)) {
    Panel::zstack()
        .id(WidgetId::from_hash(id))
        .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
        .show(ui, body);
}

/// Pin: a rounded bordered triangle and a three-color mesh, each turned 30° about its owner's
/// center. The triangle keeps its rounding and stroke width; the mesh keeps its vertex colors.
#[test]
fn a_turned_triangle_and_mesh_match_golden() {
    let mut mesh = Mesh::new();
    let a = mesh.vertex(Vec2::new(15.0, 80.0), RgbaF32::srgb(1.0, 0.3, 0.3));
    let b = mesh.vertex(Vec2::new(85.0, 80.0), RgbaF32::srgb(0.3, 1.0, 0.4));
    let c = mesh.vertex(Vec2::new(50.0, 15.0), RgbaF32::srgb(0.3, 0.5, 1.0));
    mesh.triangle(a, b, c);
    GoldenName::TurnedTriangleAndMesh.assert_scene(UVec2::new(220, 120), |ui| {
        Panel::hstack().auto_id().padding(10.0).show(ui, |ui| {
            owner(ui, "triangle", |ui| {
                ui.add_shape_animated(
                    Shape::triangle(
                        Vec2::new(15.0, 80.0),
                        Vec2::new(85.0, 80.0),
                        Vec2::new(50.0, 15.0),
                    )
                    .radius(8.0_f32)
                    .fill(RgbaF32::srgb(0.2, 0.5, 1.0))
                    .border(Stroke::new(RgbaF32::WHITE, 3.0)),
                    thirty_degrees(),
                );
            });
            owner(ui, "mesh", |ui| {
                ui.add_shape_animated(Shape::mesh(&mesh), thirty_degrees());
            });
        });
    });
}

/// Pin: each quad kind turned 30° about its owner's center. A rounded rect with a border, a
/// gradient rect whose axis turns with it, a drop shadow with its offset turned with the source,
/// an inset shadow, a square dot, the cap a curve of no length paints, and a bordered window
/// whose outer edge fades as any turned edge does. A light fill marks each shadow's source, so the
/// turned shadow shows against it.
#[test]
fn turned_quads_match_golden() {
    let source = Rect::new(20.0, 30.0, 60.0, 40.0);
    GoldenName::TurnedQuads.assert_scene(UVec2::new(670, 120), |ui| {
        Panel::hstack().auto_id().padding(10.0).show(ui, |ui| {
            owner(ui, "rounded", |ui| {
                ui.add_shape_animated(
                    Shape::rect(source)
                        .corners(Corners::all(10.0))
                        .fill(RgbaF32::srgb(0.2, 0.5, 1.0))
                        .border(Stroke::new(RgbaF32::WHITE, 3.0)),
                    thirty_degrees(),
                );
            });
            owner(ui, "gradient", |ui| {
                ui.add_shape_animated(
                    Shape::rect(source).fill(LinearGradient::two_stop(
                        0.0,
                        RgbaF32::hex(0xff5e44),
                        RgbaF32::hex(0xfacc15),
                    )),
                    thirty_degrees(),
                );
            });
            owner(ui, "drop", |ui| {
                ui.add_shape_animated(
                    Shape::shadow(Shadow::drop(RgbaF32::BLACK, Vec2::new(8.0, 8.0), 6.0))
                        .at(source)
                        .corners(Corners::all(6.0)),
                    thirty_degrees(),
                );
                ui.add_shape_animated(
                    Shape::rect(source)
                        .corners(Corners::all(6.0))
                        .fill(RgbaF32::srgb(0.85, 0.85, 0.85)),
                    thirty_degrees(),
                );
            });
            owner(ui, "inset", |ui| {
                ui.add_shape_animated(
                    Shape::rect(source)
                        .corners(Corners::all(6.0))
                        .fill(RgbaF32::srgb(0.85, 0.85, 0.85)),
                    thirty_degrees(),
                );
                ui.add_shape_animated(
                    Shape::shadow(Shadow::drop(RgbaF32::BLACK, Vec2::new(6.0, 6.0), 5.0).inset())
                        .at(source)
                        .corners(Corners::all(6.0)),
                    thirty_degrees(),
                );
            });
            owner(ui, "dot", |ui| {
                let stroke = Stroke::new(RgbaF32::srgb(1.0, 0.85, 0.2), 30.0);
                ui.add_shape_animated(
                    Shape::line(Vec2::new(50.0, 50.0), Vec2::new(50.0, 50.0), stroke)
                        .cap(LineCap::Square),
                    thirty_degrees(),
                );
            });
            owner(ui, "window", |ui| {
                ui.add_shape_animated(
                    Shape::windowed_rect(source)
                        .corners(Corners::all(14.0))
                        .fill(RgbaF32::srgb(0.2, 0.5, 1.0))
                        .border(Stroke::new(RgbaF32::WHITE, 2.0)),
                    thirty_degrees(),
                );
            });
        });
    });
}

/// Pin: a 2×2 four-color image turned 30° about its owner's center, stretched and tiled three
/// times across. The texels turn with the rect, and the turned edges fade over the AA width as a
/// quad's do.
#[test]
fn turned_images_match_golden() {
    let texels = [
        [230, 70, 60, 255],
        [60, 160, 230, 255],
        [250, 210, 60, 255],
        [80, 200, 110, 255],
    ];
    let image = Image::from_srgba8(UVec2::new(2, 2), texels.concat()).unwrap();
    let mut h = VisualRig::new();
    let handle = h.host.ui().load_image(&image).unwrap();
    let source = Rect::new(20.0, 30.0, 60.0, 40.0);
    let capture = h.size(UVec2::new(230, 120)).settled_frame(0, |ui| {
        Panel::hstack().auto_id().padding(10.0).show(ui, |ui| {
            owner(ui, "fill", |ui| {
                ui.add_shape_animated(
                    Shape::image(handle.clone()).at(source).fit(ImageFit::Fill),
                    thirty_degrees(),
                );
            });
            owner(ui, "tile", |ui| {
                ui.add_shape_animated(
                    Shape::image(handle.clone()).at(source).fit(ImageFit::Tile {
                        offset: Vec2::ZERO,
                        scale: Vec2::splat(3.0),
                    }),
                    thirty_degrees(),
                );
            });
        });
    });
    GoldenName::TurnedImages.assert_matches(&capture.image);
}

/// Pin: a label and a two-color icon, each turned 30° about its owner's center. The glyphs turn
/// as one run about the owner's center, filtered rather than drawn texel for texel; the icon keeps
/// its own colors.
#[test]
fn a_turned_label_and_icon_match_golden() {
    let atlas = icon::atlas();
    GoldenName::TurnedTextAndIcon.assert_scene(UVec2::new(220, 120), |ui| {
        let icons = ui.load_icons(Rc::clone(&atlas));
        Panel::hstack().auto_id().padding(10.0).show(ui, |ui| {
            owner(ui, "label", |ui| {
                let label = ui.intern("Turned");
                ui.add_shape_animated(
                    Shape::text(label, GlyphFont::new(20.0))
                        .color(RgbaF32::WHITE)
                        .at_origin(Vec2::new(18.0, 38.0)),
                    thirty_degrees(),
                );
            });
            owner(ui, "icon", |ui| {
                ui.add_shape_animated(
                    Shape::icon(icons.by_name("halves").expect("fixture icon"))
                        .at(Rect::new(25.0, 25.0, 50.0, 50.0))
                        .fit(IconFit::Fill),
                    thirty_degrees(),
                );
            });
        });
    });
}
