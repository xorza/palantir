//! A turn paint animation on each shape kind that turns, held at 30°.

use palantir::Fit;
use palantir::glam::{UVec2, Vec2};

use palantir::widget::{GlyphFont, Mesh, PaintAnimation, Shape};
use palantir::{
    Border, Configure, Corners, Image, ImageFit, LinearGradient, Panel, RadialGradient, Rect,
    RgbaF32, Shadow, Sizing, Ui, WidgetId,
};

use std::rc::Rc;

use crate::fixtures::icon;
use crate::golden_name::GoldenName;
use crate::visual_rig::VisualRig;
use palantir::LineCap;
use palantir::Paint;
use palantir::Stroke;
use palantir::widget::IconPaint;
use palantir::widget::ImagePaint;

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
                if let Some(shape) = ui.add_shape(
                    Shape::triangle(
                        Vec2::new(15.0, 80.0),
                        Vec2::new(85.0, 80.0),
                        Vec2::new(50.0, 15.0),
                    )
                    .radius(8.0_f32),
                    Paint::from(RgbaF32::srgb(0.2, 0.5, 1.0))
                        .with_border(Border::new(RgbaF32::WHITE, 3.0)),
                ) {
                    shape.animate(thirty_degrees());
                }
            });
            owner(ui, "mesh", |ui| {
                if let Some(shape) = ui.add_shape(Shape::mesh(&mesh), RgbaF32::WHITE) {
                    shape.animate(thirty_degrees());
                }
            });
        });
    });
}

/// Pin: a triangle's gradient spans the rounded triangle's bounds and turns with it. A linear
/// gradient on a sharp triangle and a radial one on a rounded bordered triangle, each still, then
/// the linear one turned 30°, its bands turned with its edges.
#[test]
fn triangle_gradients_match_golden() {
    let triangle = || {
        Shape::triangle(
            Vec2::new(15.0, 80.0),
            Vec2::new(85.0, 80.0),
            Vec2::new(50.0, 15.0),
        )
    };
    let linear = LinearGradient::horizontal([RgbaF32::hex(0xff5e44), RgbaF32::hex(0xfacc15)]);
    let radial = RadialGradient::centered([RgbaF32::hex(0xfacc15), RgbaF32::hex(0x2563eb)]);
    GoldenName::TriangleGradients.assert_scene(UVec2::new(320, 120), |ui| {
        Panel::hstack().auto_id().padding(10.0).show(ui, |ui| {
            owner(ui, "linear", |ui| {
                ui.add_shape(triangle(), linear.clone());
            });
            owner(ui, "radial", |ui| {
                ui.add_shape(
                    triangle().radius(10.0_f32),
                    Paint::from(radial.clone()).with_border(Border::new(RgbaF32::WHITE, 3.0)),
                );
            });
            owner(ui, "turned", |ui| {
                if let Some(shape) = ui.add_shape(triangle(), linear.clone()) {
                    shape.animate(thirty_degrees());
                }
            });
        });
    });
}

/// Pin: each quad kind turned 30° about its owner's center. A rounded rect with a border, a
/// gradient rect whose axis turns with it, a drop shadow with its offset turned with the source,
/// an inset shadow, and a square dot, the cap a curve of no length paints. A light fill marks each
/// shadow's source, so the turned shadow shows against it.
#[test]
fn turned_quads_match_golden() {
    let source = Rect::new(20.0, 30.0, 60.0, 40.0);
    GoldenName::TurnedQuads.assert_scene(UVec2::new(670, 120), |ui| {
        Panel::hstack().auto_id().padding(10.0).show(ui, |ui| {
            owner(ui, "rounded", |ui| {
                if let Some(shape) = ui.add_shape(
                    Shape::rect().at(source).corners(Corners::all(10.0)),
                    Paint::from(RgbaF32::srgb(0.2, 0.5, 1.0))
                        .with_border(Border::new(RgbaF32::WHITE, 3.0)),
                ) {
                    shape.animate(thirty_degrees());
                }
            });
            owner(ui, "gradient", |ui| {
                if let Some(shape) = ui.add_shape(
                    Shape::rect().at(source),
                    LinearGradient::horizontal([RgbaF32::hex(0xff5e44), RgbaF32::hex(0xfacc15)]),
                ) {
                    shape.animate(thirty_degrees());
                }
            });
            owner(ui, "drop", |ui| {
                if let Some(shape) = ui.add_shape(
                    Shape::rect().at(source).corners(Corners::all(6.0)),
                    Shadow::drop(RgbaF32::BLACK, Vec2::new(8.0, 8.0), 6.0),
                ) {
                    shape.animate(thirty_degrees());
                }
                if let Some(shape) = ui.add_shape(
                    Shape::rect().at(source).corners(Corners::all(6.0)),
                    RgbaF32::srgb(0.85, 0.85, 0.85),
                ) {
                    shape.animate(thirty_degrees());
                }
            });
            owner(ui, "inset", |ui| {
                if let Some(shape) = ui.add_shape(
                    Shape::rect().at(source).corners(Corners::all(6.0)),
                    RgbaF32::srgb(0.85, 0.85, 0.85),
                ) {
                    shape.animate(thirty_degrees());
                }
                if let Some(shape) = ui.add_shape(
                    Shape::rect().at(source).corners(Corners::all(6.0)),
                    Shadow::inset(RgbaF32::BLACK, Vec2::new(6.0, 6.0), 5.0),
                ) {
                    shape.animate(thirty_degrees());
                }
            });
            owner(ui, "dot", |ui| {
                if let Some(shape) = ui.add_shape(
                    Shape::line(Vec2::new(50.0, 50.0), Vec2::new(50.0, 50.0)),
                    Stroke::new(RgbaF32::srgb(1.0, 0.85, 0.2), 30.0).with_cap(LineCap::Square),
                ) {
                    shape.animate(thirty_degrees());
                }
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
                if let Some(shape) = ui.add_shape(
                    Shape::rect().at(source),
                    ImagePaint::new(handle.clone()).fit(ImageFit::Fill),
                ) {
                    shape.animate(thirty_degrees());
                }
            });
            owner(ui, "tile", |ui| {
                if let Some(shape) = ui.add_shape(
                    Shape::rect().at(source),
                    ImagePaint::new(handle.clone()).fit(ImageFit::Tile {
                        offset: Vec2::ZERO,
                        scale: Vec2::splat(3.0),
                    }),
                ) {
                    shape.animate(thirty_degrees());
                }
            });
        });
    });
    GoldenName::TurnedImages.assert_matches(&capture.image);
}

/// Pin: a rect's corners clip its image. A 2×2 four-color image stretched under corners of 12, a
/// contained one in a wider rounded rect, which shows nothing past its own square, its sides faded
/// there, and the stretched one turned 30°, its corners turned with it.
#[test]
fn rounded_images_match_golden() {
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
    let rounded = Shape::rect().at(source).corners(Corners::all(12.0));
    let capture = h.size(UVec2::new(340, 120)).settled_frame(0, |ui| {
        Panel::hstack().auto_id().padding(10.0).show(ui, |ui| {
            owner(ui, "fill", |ui| {
                ui.add_shape(rounded, ImagePaint::new(handle.clone()).fit(ImageFit::Fill));
            });
            owner(ui, "contain", |ui| {
                ui.add_shape(
                    rounded,
                    ImagePaint::new(handle.clone()).fit(ImageFit::Contain),
                );
            });
            owner(ui, "turned", |ui| {
                if let Some(shape) =
                    ui.add_shape(rounded, ImagePaint::new(handle.clone()).fit(ImageFit::Fill))
                {
                    shape.animate(thirty_degrees());
                }
            });
        });
    });
    GoldenName::RoundedImages.assert_matches(&capture.image);
}

/// Pin: a triangle clips its image, which fits its covering rect. A 2×2 four-color image stretched
/// over a rounded triangle, contained in a wide one, which shows nothing past the image's own
/// square, and the stretched one turned 30°, the image turned with it.
#[test]
fn triangle_images_match_golden() {
    let texels = [
        [230, 70, 60, 255],
        [60, 160, 230, 255],
        [250, 210, 60, 255],
        [80, 200, 110, 255],
    ];
    let image = Image::from_srgba8(UVec2::new(2, 2), texels.concat()).unwrap();
    let mut h = VisualRig::new();
    let handle = h.host.ui().load_image(&image).unwrap();
    let rounded = Shape::triangle(
        Vec2::new(15.0, 80.0),
        Vec2::new(85.0, 80.0),
        Vec2::new(50.0, 15.0),
    )
    .radius(8.0_f32);
    let wide = Shape::triangle(
        Vec2::new(5.0, 70.0),
        Vec2::new(95.0, 70.0),
        Vec2::new(50.0, 30.0),
    );
    let capture = h.size(UVec2::new(340, 120)).settled_frame(0, |ui| {
        Panel::hstack().auto_id().padding(10.0).show(ui, |ui| {
            owner(ui, "fill", |ui| {
                ui.add_shape(
                    rounded.clone(),
                    ImagePaint::new(handle.clone()).fit(ImageFit::Fill),
                );
            });
            owner(ui, "contain", |ui| {
                ui.add_shape(
                    wide.clone(),
                    ImagePaint::new(handle.clone()).fit(ImageFit::Contain),
                );
            });
            owner(ui, "turned", |ui| {
                if let Some(shape) = ui.add_shape(
                    rounded.clone(),
                    ImagePaint::new(handle.clone()).fit(ImageFit::Fill),
                ) {
                    shape.animate(thirty_degrees());
                }
            });
        });
    });
    GoldenName::TriangleImages.assert_matches(&capture.image);
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
                if let Some(shape) = ui.add_shape(
                    Shape::text(label, GlyphFont::new(20.0)).at_origin(Vec2::new(18.0, 38.0)),
                    RgbaF32::WHITE,
                ) {
                    shape.animate(thirty_degrees());
                }
            });
            owner(ui, "icon", |ui| {
                if let Some(shape) = ui.add_shape(
                    Shape::rect().at(Rect::new(25.0, 25.0, 50.0, 50.0)),
                    IconPaint::new(icons.by_name("halves").expect("fixture icon")).fit(Fit::Fill),
                ) {
                    shape.animate(thirty_degrees());
                }
            });
        });
    });
}
