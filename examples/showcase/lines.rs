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
    // Same sharp chevron painted twice with different join modes.
    // Top: Miter (auto-falls-back to bevel at MITER_LIMIT — sharp
    // angle here exceeds it, so this is effectively a bevel).
    // Middle: Bevel (forced — visually same as the top one because
    // the auto-fallback kicked in).
    // Bottom: a shallow 90° corner with Miter — actually mitres to
    // a sharp point. Side-by-side comparison shows when each mode
    // wins.
    let cyan = Color::rgb(0.2, 0.9, 1.0);
    let chevron_sharp_a = [
        Vec2::new(10.0, 18.0),
        Vec2::new(70.0, 28.0),
        Vec2::new(15.0, 38.0),
    ];
    ui.add_shape(Shape::Polyline {
        points: &chevron_sharp_a,
        colors: PolylineColors::Single(cyan),
        width: 4.0,
        cap: LineCap::Butt,
        join: LineJoin::Miter,
    });
    let chevron_sharp_b = [
        Vec2::new(10.0, 58.0),
        Vec2::new(70.0, 68.0),
        Vec2::new(15.0, 78.0),
    ];
    ui.add_shape(Shape::Polyline {
        points: &chevron_sharp_b,
        colors: PolylineColors::Single(cyan),
        width: 4.0,
        cap: LineCap::Butt,
        join: LineJoin::Bevel,
    });
    let chevron_shallow = [
        Vec2::new(10.0, 100.0),
        Vec2::new(60.0, 115.0),
        Vec2::new(110.0, 100.0),
    ];
    ui.add_shape(Shape::Polyline {
        points: &chevron_shallow,
        colors: PolylineColors::Single(cyan),
        width: 4.0,
        cap: LineCap::Butt,
        join: LineJoin::Miter,
    });
}

fn caps(ui: &mut Ui) {
    // Two pairs of identical lines, top with Butt caps, bottom
    // with Square. The Square pair visibly extends past its
    // endpoints by half the width — flat-end against a backdrop
    // marker line makes the extension obvious.
    let red = Color::rgb(1.0, 0.4, 0.4);
    let green = Color::rgb(0.4, 1.0, 0.4);
    // Marker rect endpoints (paint a thin vertical line behind
    // both pairs so the cap extension is visible).
    let marker = Color::rgb(1.0, 1.0, 1.0);
    for y in [40.0_f32, 90.0] {
        ui.add_shape(Shape::Line {
            a: Vec2::new(30.0, y - 15.0),
            b: Vec2::new(30.0, y + 15.0),
            width: 1.0,
            color: marker,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
        });
        ui.add_shape(Shape::Line {
            a: Vec2::new(90.0, y - 15.0),
            b: Vec2::new(90.0, y + 15.0),
            width: 1.0,
            color: marker,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
        });
    }
    // Butt cap row (top): stroke ends at the marker.
    ui.add_shape(Shape::Line {
        a: Vec2::new(30.0, 40.0),
        b: Vec2::new(90.0, 40.0),
        width: 8.0,
        color: red,
        cap: LineCap::Butt,
        join: LineJoin::Miter,
    });
    // Square cap row (bottom): stroke extends past the marker by half-width.
    ui.add_shape(Shape::Line {
        a: Vec2::new(30.0, 90.0),
        b: Vec2::new(90.0, 90.0),
        width: 8.0,
        color: green,
        cap: LineCap::Square,
        join: LineJoin::Miter,
    });
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
