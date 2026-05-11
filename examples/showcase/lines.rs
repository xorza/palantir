use glam::Vec2;
use palantir::{Color, Configure, Panel, PolylineColors, Shape, Sizing, Ui};

pub fn build(ui: &mut Ui) {
    Panel::hstack()
        .auto_id()
        .gap(24.0)
        .padding(24.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            cell(ui, "widths", widths);
            cell(ui, "hairlines", hairlines);
            cell(ui, "gradient", gradient);
            cell(ui, "per_segment", per_segment);
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

fn gradient(ui: &mut Ui) {
    // PerPoint coloring on a zig-zag polyline. GPU lerps between
    // adjacent cross-sections, giving a smooth multi-stop gradient
    // along the stroke — single shape, no per-segment recording.
    let pts = [
        Vec2::new(10.0, 10.0),
        Vec2::new(40.0, 110.0),
        Vec2::new(70.0, 30.0),
        Vec2::new(110.0, 110.0),
    ];
    let cols = [
        Color::rgb(1.0, 0.2, 0.2),
        Color::rgb(1.0, 0.85, 0.2),
        Color::rgb(0.2, 1.0, 0.4),
        Color::rgb(0.2, 0.6, 1.0),
    ];
    ui.add_shape(Shape::Polyline {
        points: &pts,
        colors: PolylineColors::PerPoint(&cols),
        width: 4.0,
    });
}

fn per_segment(ui: &mut Ui) {
    // PerSegment paints each segment in a solid block — interior
    // cross-sections duplicate so colors don't bleed at joins.
    // Six-segment polyline cycles through three colors twice.
    let pts = [
        Vec2::new(10.0, 60.0),
        Vec2::new(30.0, 30.0),
        Vec2::new(50.0, 90.0),
        Vec2::new(70.0, 30.0),
        Vec2::new(90.0, 90.0),
        Vec2::new(110.0, 30.0),
        Vec2::new(110.0, 90.0),
    ];
    let cols = [
        Color::rgb(1.0, 0.2, 0.2),
        Color::rgb(1.0, 0.85, 0.2),
        Color::rgb(0.2, 1.0, 0.4),
        Color::rgb(0.2, 0.6, 1.0),
        Color::rgb(0.7, 0.3, 1.0),
        Color::rgb(1.0, 0.5, 0.8),
    ];
    ui.add_shape(Shape::Polyline {
        points: &pts,
        colors: PolylineColors::PerSegment(&cols),
        width: 4.0,
    });
}
