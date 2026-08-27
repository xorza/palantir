//! The shape specimen sheet — one captioned canvas per authoring shape
//! family. This is where the fixture's *shape-level* coverage lives: the
//! rest of the tree paints through widget chrome, so without these cells
//! `Shape::triangle` / `curve` / `polyline` / `mesh` / `shadow` and the
//! shape-level `Brush` variants would never be recorded at all.

use std::f32::consts::PI;

use crate::demo_swatches;
use crate::frame_fixture::tokens;
use crate::layout::types::sizing::Sizing;
use crate::primitives::brush::gradient::conic_geometry::ConicGradient;
use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::brush::gradient::radial_geometry::RadialGradient;
use crate::primitives::brush::gradient::stops::Stop;
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::scene::node::Configure;
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use crate::shape::style::{LineCap, LineJoin};
use crate::ui::Ui;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;

/// Specimen sheet: one captioned canvas per shape family, tiled through a
/// `WrapHStack` so the sheet reflows to whatever width the column has
/// instead of clumping in a corner.
pub(super) fn sheet(ui: &mut Ui) {
    tokens::card(ui, "specimens", "SHAPES", Sizing::HUG, |ui| {
        Panel::wrap_hstack()
            .id_salt("specimen-wrap")
            .gap(8.0)
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                cell(ui, "brushes", "brushes", add_brush_swatches);
                cell(ui, "curves", "curves", add_curves);
                cell(ui, "polyline", "polyline", add_polyline);
                cell(ui, "arcs", "arc / circle", add_arcs);
                cell(ui, "solids", "triangle / mesh", add_solids);
                cell(ui, "shadow", "shadow", add_shadow);
            });
    });
}

/// One 168x96 specimen cell: caption over a recessed canvas. Shapes the
/// body adds are canvas-local, so every cell shares one coordinate box.
fn cell(ui: &mut Ui, id: &'static str, label: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .id_salt(("spec", id))
        .gap(4.0)
        .size((Sizing::fixed(168.0), Sizing::HUG))
        .show(ui, |ui| {
            Text::new(label)
                .id_salt(("spec-cap", id))
                .style(&tokens::caption_style())
                .show(ui);
            Panel::canvas()
                .id_salt(("spec-canvas", id))
                .size((Sizing::FILL, Sizing::fixed(96.0)))
                .background(tokens::well_bg())
                .show(ui, body);
        });
}

/// Shape-level (not chrome-level) Solid / Linear / Radial / Conic fills.
fn add_brush_swatches(ui: &mut Ui) {
    ui.add_shape(
        Shape::rect(Rect::new(6.0, 6.0, 74.0, 38.0))
            .corners(6.0)
            .fill(Color::hex(0xd9544c))
            .stroke(Stroke::solid(Color::rgba(1.0, 1.0, 1.0, 0.5), 1.0)),
    );
    ui.add_shape(
        Shape::rect(Rect::new(88.0, 6.0, 74.0, 38.0))
            .corners(6.0)
            .fill(
                LinearGradient::builder(PI / 2.0)
                    .stop(0.0, ColorU8::hex(0x1a1a2e))
                    .stop(1.0, ColorU8::hex(0x4c5cdb)),
            ),
    );
    ui.add_shape(
        Shape::rect(Rect::new(6.0, 52.0, 74.0, 38.0))
            .corners(6.0)
            .fill(RadialGradient::two_stop_centered(
                ColorU8::hex(0xfacc15),
                ColorU8::hex(0x1a1a2e),
            )),
    );
    ui.add_shape(
        Shape::rect(Rect::new(88.0, 52.0, 74.0, 38.0))
            .corners(6.0)
            .fill(ConicGradient::new(
                glam::Vec2::splat(0.5),
                0.0,
                [
                    Stop::new(0.0, ColorU8::hex(0xff5e44)),
                    Stop::new(0.5, ColorU8::hex(0x46c46c)),
                    Stop::new(1.0, ColorU8::hex(0x4c5cdb)),
                ],
            )),
    );
}

fn add_curves(ui: &mut Ui) {
    ui.add_shape(
        Shape::line(
            glam::Vec2::new(12.0, 16.0),
            glam::Vec2::new(156.0, 28.0),
            3.0,
        )
        .brush(tokens::ACCENT)
        .cap(LineCap::Round),
    );
    ui.add_shape(
        Shape::quadratic_bezier(
            glam::Vec2::new(12.0, 50.0),
            glam::Vec2::new(84.0, 22.0),
            glam::Vec2::new(156.0, 50.0),
            3.0,
        )
        .brush(tokens::WARN)
        .cap(LineCap::Square),
    );
    ui.add_shape(
        Shape::cubic_bezier(
            glam::Vec2::new(12.0, 86.0),
            glam::Vec2::new(56.0, 56.0),
            glam::Vec2::new(112.0, 96.0),
            glam::Vec2::new(156.0, 68.0),
            4.0,
        )
        .brush(tokens::OK)
        .cap(LineCap::Round),
    );
}

fn add_polyline(ui: &mut Ui) {
    let pts: [glam::Vec2; 6] = [
        glam::Vec2::new(14.0, 74.0),
        glam::Vec2::new(42.0, 32.0),
        glam::Vec2::new(70.0, 66.0),
        glam::Vec2::new(98.0, 26.0),
        glam::Vec2::new(126.0, 62.0),
        glam::Vec2::new(154.0, 38.0),
    ];
    // The shared swatch set in order, plus one green it doesn't carry —
    // six points need six distinguishable inks.
    let cols = [
        demo_swatches::RED,
        demo_swatches::ORANGE,
        demo_swatches::LIME,
        Color::hex(0x46c46c),
        demo_swatches::TEAL,
        demo_swatches::VIOLET,
    ];
    ui.add_shape(Shape::polyline(&pts, PolylineColors::PerPoint(&cols), 4.0).join(LineJoin::Round));
}

fn add_arcs(ui: &mut Ui) {
    ui.add_shape(
        Shape::arc(glam::Vec2::new(84.0, 78.0), 40.0, PI, PI, 5.0)
            .brush(tokens::ACCENT)
            .cap(LineCap::Round),
    );
    ui.add_shape(Shape::circle(glam::Vec2::new(84.0, 26.0), 12.0, 3.0).brush(tokens::VIOLET));
}

fn add_solids(ui: &mut Ui) {
    ui.add_shape(
        Shape::triangle(
            glam::Vec2::new(18.0, 82.0),
            glam::Vec2::new(52.0, 20.0),
            glam::Vec2::new(86.0, 82.0),
        )
        .fill(tokens::WARN)
        .radius(2.0_f32),
    );
    ui.add_shape(Shape::mesh(gradient_mesh()));
}

/// A `Shape::shadow` under the rect that casts it — the shape-level peer
/// of the chrome shadow on [`tokens::card_bg`].
fn add_shadow(ui: &mut Ui) {
    let plate = Rect::new(34.0, 20.0, 100.0, 52.0);
    ui.add_shape(
        Shape::shadow(Shadow::drop(
            Color::rgba(0.0, 0.0, 0.0, 0.75),
            glam::Vec2::new(0.0, 7.0),
            9.0,
        ))
        .at(plate)
        .corners(10.0),
    );
    ui.add_shape(
        Shape::rect(plate)
            .corners(10.0)
            .fill(Color::hex(0x30364a))
            .stroke(Stroke::solid(tokens::BORDER, 1.0)),
    );
}

/// The mesh payload is built once and leaked: `Shape::mesh` borrows a
/// `&'static Mesh`, and rebuilding it per frame would allocate — which the
/// `record-only` alloc step forbids.
fn gradient_mesh() -> &'static Mesh {
    use std::sync::OnceLock;
    static MESH_PTR: OnceLock<usize> = OnceLock::new();
    unsafe {
        &*(*MESH_PTR.get_or_init(|| {
            let mut m = Mesh::new();
            let a = m.vertex(glam::Vec2::new(96.0, 82.0), ColorU8::hex(0xff5e44));
            let b = m.vertex(glam::Vec2::new(128.0, 22.0), ColorU8::hex(0xfacc15));
            let c = m.vertex(glam::Vec2::new(160.0, 82.0), ColorU8::hex(0x46c46c));
            m.triangle(a, b, c);
            Box::into_raw(Box::new(m)) as usize
        }) as *const Mesh)
    }
}
