use super::app_state::AppState;
use glam::Vec2;
use palantir::{Color, Configure, LineCap, LineJoin, Panel, Shape, Sizing, Ui};

pub fn build(ui: &mut Ui<AppState>) {
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
                    cell(ui, "tolerance", tolerance_sweep);
                });
        });
}

fn cell<T>(ui: &mut Ui<T>, id: &'static str, paint: impl Fn(&mut Ui<T>)) {
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

fn cubic<T>(ui: &mut Ui<T>) {
    ui.add_shape(Shape::CubicBezier {
        p0: P0,
        p1: P1,
        p2: P2,
        p3: P3,
        width: 4.0,
        brush: Color::rgb(0.2, 0.9, 1.0).into(),
        cap: LineCap::Butt,
        join: LineJoin::Miter,
        tolerance: 0.25,
    });
}

fn quadratic<T>(ui: &mut Ui<T>) {
    ui.add_shape(Shape::QuadraticBezier {
        p0: Q0,
        p1: Q1,
        p2: Q2,
        width: 4.0,
        brush: Color::rgb(0.4, 1.0, 0.5).into(),
        cap: LineCap::Butt,
        join: LineJoin::Miter,
        tolerance: 0.25,
    });
}

fn tolerance_sweep<T>(ui: &mut Ui<T>) {
    // Same curve at three flattening tolerances. Loose tolerances
    // produce visibly polygonal output; tight ones look smooth.
    for (i, tol) in [4.0_f32, 1.0, 0.25].iter().enumerate() {
        let dy = i as f32 * 35.0;
        ui.add_shape(Shape::CubicBezier {
            p0: Vec2::new(10.0, 25.0 + dy),
            p1: Vec2::new(35.0, 5.0 + dy),
            p2: Vec2::new(85.0, 45.0 + dy),
            p3: Vec2::new(110.0, 25.0 + dy),
            width: 3.0,
            brush: Color::rgb(1.0, 0.85, 0.2).into(),
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            tolerance: *tol,
        });
    }
}
