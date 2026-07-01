use glam::Vec2;
use palantir::{Color, Configure, Panel, Shape, Sizing, Stroke, Ui};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Panel::hstack()
                .id_salt("row1")
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    cell(ui, "sharp", sharp);
                    cell(ui, "rounded", rounded);
                    cell(ui, "stroked", stroked);
                });
            Panel::hstack()
                .id_salt("row2")
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    cell(ui, "outline", outline);
                    cell(ui, "radii", radii);
                });
        });
}

fn cell(ui: &mut Ui, id: &'static str, paint: impl Fn(&mut Ui)) {
    Panel::zstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(8.0)
        .show(ui, paint);
}

const A: Vec2 = Vec2::new(15.0, 100.0);
const B: Vec2 = Vec2::new(60.0, 15.0);
const C: Vec2 = Vec2::new(105.0, 100.0);

/// Sharp-cornered solid fill — the aliased case a `Mesh::filled_triangle`
/// would give, now with crisp SDF coverage AA.
fn sharp(ui: &mut Ui) {
    ui.add_shape(Shape::Triangle {
        a: A,
        b: B,
        c: C,
        radius: 0.0,
        fill: Color::rgb(0.2, 0.9, 1.0).into(),
        stroke: Stroke::ZERO,
    });
}

/// Rounded corners — `SDF - radius`, no extra geometry.
fn rounded(ui: &mut Ui) {
    ui.add_shape(Shape::Triangle {
        a: A,
        b: B,
        c: C,
        radius: 12.0,
        fill: Color::rgb(0.4, 1.0, 0.5).into(),
        stroke: Stroke::ZERO,
    });
}

/// Fill + inner-edge stroke, rounded.
fn stroked(ui: &mut Ui) {
    ui.add_shape(Shape::Triangle {
        a: A,
        b: B,
        c: C,
        radius: 10.0,
        fill: Color::rgb(0.2, 0.5, 1.0).into(),
        stroke: Stroke::solid(Color::WHITE, 3.0),
    });
}

/// Stroke only (transparent fill) — a rounded triangular outline.
fn outline(ui: &mut Ui) {
    ui.add_shape(Shape::Triangle {
        a: A,
        b: B,
        c: C,
        radius: 8.0,
        fill: Color::TRANSPARENT.into(),
        stroke: Stroke::solid(Color::rgb(1.0, 0.85, 0.2), 3.0),
    });
}

/// A play-triangle (▶) at three corner radii — the toolbar-glyph use case,
/// from sharp to increasingly soft.
fn radii(ui: &mut Ui) {
    for (i, r) in [0.0_f32, 4.0, 10.0].iter().enumerate() {
        let dx = i as f32 * 40.0;
        ui.add_shape(Shape::Triangle {
            a: Vec2::new(10.0 + dx, 20.0),
            b: Vec2::new(10.0 + dx, 60.0),
            c: Vec2::new(38.0 + dx, 40.0),
            radius: *r,
            fill: Color::rgb(1.0, 0.6, 0.3).into(),
            stroke: Stroke::ZERO,
        });
    }
}
