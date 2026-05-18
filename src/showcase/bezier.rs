use glam::Vec2;
use palantir::{Color, Configure, LineCap, Panel, Shape, Sizing, Ui};

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
                    cell(ui, "cubic", cubic);
                    cell(ui, "quadratic", quadratic);
                    cell(ui, "caps", cap_variants);
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

const P0: Vec2 = Vec2::new(10.0, 100.0);
const P1: Vec2 = Vec2::new(35.0, 10.0);
const P2: Vec2 = Vec2::new(85.0, 10.0);
const P3: Vec2 = Vec2::new(110.0, 100.0);

const Q0: Vec2 = Vec2::new(10.0, 100.0);
const Q1: Vec2 = Vec2::new(60.0, 5.0);
const Q2: Vec2 = Vec2::new(110.0, 100.0);

fn cubic(ui: &mut Ui) {
    ui.add_shape(Shape::CubicBezier {
        p0: P0,
        p1: P1,
        p2: P2,
        p3: P3,
        width: 4.0,
        brush: Color::rgb(0.2, 0.9, 1.0).into(),
        cap: LineCap::Butt,
    });
}

fn quadratic(ui: &mut Ui) {
    ui.add_shape(Shape::QuadraticBezier {
        p0: Q0,
        p1: Q1,
        p2: Q2,
        width: 4.0,
        brush: Color::rgb(0.4, 1.0, 0.5).into(),
        cap: LineCap::Butt,
    });
}

fn cap_variants(ui: &mut Ui) {
    // Three identical curves, one per cap kind — the endpoint shape
    // is the only visual delta. Mirrors `curve_caps_match_golden`.
    for (i, cap) in [LineCap::Butt, LineCap::Square, LineCap::Round]
        .iter()
        .enumerate()
    {
        let dy = i as f32 * 35.0;
        ui.add_shape(Shape::CubicBezier {
            p0: Vec2::new(10.0, 25.0 + dy),
            p1: Vec2::new(35.0, 5.0 + dy),
            p2: Vec2::new(85.0, 45.0 + dy),
            p3: Vec2::new(110.0, 25.0 + dy),
            width: 8.0,
            brush: Color::rgb(1.0, 0.85, 0.2).into(),
            cap: *cap,
        });
    }
}
