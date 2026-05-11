use glam::Vec2;
use palantir::{Color, Configure, LineCap, LineJoin, Panel, PolylineColors, Shape, Sizing, Ui};

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
                    cell(ui, "widths", widths);
                    cell(ui, "hairlines", hairlines);
                    cell(ui, "gradient", gradient);
                });
            Panel::hstack()
                .id_salt("row2")
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    cell(ui, "per_segment", per_segment);
                    cell(ui, "joins", joins);
                    cell(ui, "caps", caps);
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

fn widths(ui: &mut Ui) {
    let cyan = Color::rgb(0.2, 0.9, 1.0);
    for (i, w) in [1.0_f32, 2.0, 3.0, 5.0, 8.0].iter().enumerate() {
        let y = 12.0 + i as f32 * 20.0;
        ui.add_shape(Shape::Line {
            a: Vec2::new(10.0, y),
            b: Vec2::new(110.0, y),
            width: *w,
            color: cyan,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
        });
    }
}

fn hairlines(ui: &mut Ui) {
    let white = Color::rgb(1.0, 1.0, 1.0);
    for (i, w) in [0.1_f32, 0.25, 0.5, 0.75, 1.0].iter().enumerate() {
        let y = 12.0 + i as f32 * 20.0;
        ui.add_shape(Shape::Line {
            a: Vec2::new(10.0, y),
            b: Vec2::new(110.0, y),
            width: *w,
            color: white,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
        });
    }
}

fn gradient(ui: &mut Ui) {
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
        cap: LineCap::Butt,
        join: LineJoin::Miter,
    });
}

fn joins(ui: &mut Ui) {
    // Same 90° corner painted three times to highlight join styles
    // at a non-clamp angle (where Miter actually mitres rather
    // than falling back to bevel). Row 1: Miter (sharp point).
    // Row 2: Bevel (flat cut). Row 3: Round (curved arc).
    let cyan = Color::rgb(0.2, 0.9, 1.0);
    for (y, join) in [
        (15.0_f32, LineJoin::Miter),
        (50.0, LineJoin::Bevel),
        (85.0, LineJoin::Round),
    ] {
        let pts = [
            Vec2::new(15.0, y + 25.0),
            Vec2::new(60.0, y),
            Vec2::new(105.0, y + 25.0),
        ];
        ui.add_shape(Shape::Polyline {
            points: &pts,
            colors: PolylineColors::Single(cyan),
            width: 5.0,
            cap: LineCap::Butt,
            join,
        });
    }
}

fn caps(ui: &mut Ui) {
    // Three lines, one per cap style: Butt, Square, Round. All
    // share the same endpoints; the marker rules at the ends make
    // the difference visible — Butt stops at the marker, Square
    // extends by half-width past it, Round adds a half-disc.
    let red = Color::rgb(1.0, 0.4, 0.4);
    let green = Color::rgb(0.4, 1.0, 0.4);
    let blue = Color::rgb(0.4, 0.6, 1.0);
    let marker = Color::rgb(1.0, 1.0, 1.0);
    for y in [25.0_f32, 60.0, 95.0] {
        for x in [30.0_f32, 90.0] {
            ui.add_shape(Shape::Line {
                a: Vec2::new(x, y - 12.0),
                b: Vec2::new(x, y + 12.0),
                width: 1.0,
                color: marker,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
            });
        }
    }
    for (y, color, cap) in [
        (25.0_f32, red, LineCap::Butt),
        (60.0, green, LineCap::Square),
        (95.0, blue, LineCap::Round),
    ] {
        ui.add_shape(Shape::Line {
            a: Vec2::new(30.0, y),
            b: Vec2::new(90.0, y),
            width: 8.0,
            color,
            cap,
            join: LineJoin::Miter,
        });
    }
}

fn per_segment(ui: &mut Ui) {
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
        cap: LineCap::Butt,
        join: LineJoin::Miter,
    });
}
