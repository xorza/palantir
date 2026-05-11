use glam::Vec2;
use palantir::{Color, Configure, Panel, Shape, Sizing, Ui};

pub fn build(ui: &mut Ui) {
    Panel::hstack()
        .auto_id()
        .gap(24.0)
        .padding(24.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            cell(ui, "widths", widths);
            cell(ui, "hairlines", hairlines);
            cell(ui, "angles", angles);
            cell(ui, "fan", fan);
        });
}

fn cell(ui: &mut Ui, id: &'static str, paint: impl Fn(&mut Ui)) {
    Panel::zstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(8.0)
        .show(ui, paint);
}

fn widths(ui: &mut Ui) {
    // Stack of horizontal lines at increasing widths, exercising
    // the normal fringe-AA regime.
    let cyan = Color::rgb(0.2, 0.9, 1.0);
    for (i, w) in [1.0_f32, 2.0, 3.0, 5.0, 8.0].iter().enumerate() {
        let y = 12.0 + i as f32 * 20.0;
        ui.add_shape(Shape::Line {
            a: Vec2::new(10.0, y),
            b: Vec2::new(110.0, y),
            width: *w,
            color: cyan,
        });
    }
}

fn hairlines(ui: &mut Ui) {
    // Sub-pixel widths fade via alpha — geometry stays 1 phys px.
    // A naive "snap up to 1 px" would show identical thickness for
    // every row; the unified hairline path makes them visibly dim
    // top-to-bottom.
    let white = Color::rgb(1.0, 1.0, 1.0);
    for (i, w) in [0.1_f32, 0.25, 0.5, 0.75, 1.0].iter().enumerate() {
        let y = 12.0 + i as f32 * 20.0;
        ui.add_shape(Shape::Line {
            a: Vec2::new(10.0, y),
            b: Vec2::new(110.0, y),
            width: *w,
            color: white,
        });
    }
}

fn angles(ui: &mut Ui) {
    // Spoke pattern from a center point — exposes AA quality at
    // every orientation. Axis-aligned spokes should look identical
    // to diagonal ones (modulo a +0.5-px shift in either direction).
    let cx = 60.0_f32;
    let cy = 60.0_f32;
    let r = 50.0_f32;
    let yellow = Color::rgb(1.0, 0.85, 0.2);
    for i in 0..12 {
        let theta = i as f32 * std::f32::consts::TAU / 12.0;
        ui.add_shape(Shape::Line {
            a: Vec2::new(cx, cy),
            b: Vec2::new(cx + r * theta.cos(), cy + r * theta.sin()),
            width: 2.0,
            color: yellow,
        });
    }
}

fn fan(ui: &mut Ui) {
    // A fan from one corner. Many short lines at varied angles —
    // stress-tests per-segment normal computation and the bbox cull.
    let pink = Color::rgb(1.0, 0.42, 0.72);
    for i in 0..20 {
        let t = i as f32 / 19.0;
        ui.add_shape(Shape::Line {
            a: Vec2::new(10.0, 10.0),
            b: Vec2::new(10.0 + 100.0 * t, 110.0),
            width: 1.5,
            color: pink,
        });
    }
}
