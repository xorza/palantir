//! Regression fixtures — deliberately colliding, occluding, or
//! minimal content that pins framework behavior by eye. Quarantined
//! here so the deliberately-ugly visuals don't leak into the widget
//! pages.
//!
//! - **id collisions**: rows reuse the same `.id_salt(...)` across
//!   siblings; the framework disambiguates (state survives) and the
//!   always-on overlay paints a magenta 3 px outline over offenders.
//! - **text z-order**: paint order honored across quads + text — the
//!   composer splits draw groups on every text→quad transition.
//! - **chrome concentricity**: rounded rect in rounded rect, inner
//!   radius shrunk by the stroke inset so corners stay concentric.
//! - **premultiplied alpha**: translucent polylines over a magenta
//!   backdrop; correct blending yields muted mixes, the historical
//!   straight-alpha-into-premul bug yields over-bright colors (see
//!   `docs/review-wgsl-shaders.md` A1).

use crate::support;
use crate::support::{captioned_cell, demo_cell, section, swatch_bg};
use aperture::{
    Align, Background, Button, Color, Configure, Corners, Frame, Panel, PolylineColors, Rect,
    Shape, Sizing, Stroke, Text, TextStyle, Ui,
};
use glam::Vec2;

pub fn build(ui: &mut Ui) {
    support::page(ui, |ui| {
        support::header(
            ui,
            "Regression fixtures — id-collision overlay, text z-order, chrome \
             concentricity, and premultiplied-alpha repros.",
        );

        section(
            ui,
            "idcol",
            "id collisions — first two rows reuse an explicit id across siblings and get the \
             magenta outline; the third row is clean",
            |ui| {
                support::row(ui, "idcol-r1", |ui| {
                    Button::new()
                        .id_salt("idcol-dup-btn")
                        .label("dup A")
                        .show(ui);
                    Button::new()
                        .id_salt("idcol-dup-btn")
                        .label("dup B")
                        .show(ui);
                    Button::new()
                        .id_salt("idcol-dup-btn")
                        .label("dup C")
                        .show(ui);
                    Frame::new()
                        .background(Background::fill(Color::hex(0x3a4a5c)))
                        .id_salt("idcol-dup-frame")
                        .size(36.0)
                        .show(ui);
                    Frame::new()
                        .background(Background::fill(Color::hex(0xddaa44)))
                        .id_salt("idcol-dup-frame")
                        .size(36.0)
                        .show(ui);
                });
                support::row(ui, "idcol-r2", |ui| {
                    Button::new()
                        .id_salt("idcol-clean-a")
                        .label("clean A")
                        .show(ui);
                    Button::new()
                        .id_salt("idcol-clean-b")
                        .label("clean B")
                        .show(ui);
                });
            },
        );

        Panel::hstack()
            .id_salt("zorder-row")
            .gap(12.0)
            .size((Sizing::FILL, Sizing::Fixed(200.0)))
            .show(ui, |ui| {
                zorder_cell(ui, "z-order — text on top of an earlier quad", false);
                zorder_cell(ui, "z-order — quad declared AFTER text covers it", true);
            });

        Panel::hstack()
            .id_salt("fixture-row")
            .gap(12.0)
            .size((Sizing::FILL, Sizing::Fixed(190.0)))
            .show(ui, |ui| {
                captioned_cell(ui, "chrome concentricity", chrome_concentricity);
                demo_cell(ui, "premul — solid α 0.5 (expect grey)", translucent_solid);
                demo_cell(ui, "premul — per-point α 0.5", translucent_per_point);
                demo_cell(
                    ui,
                    "premul — α 0.25 (expect slight tint)",
                    translucent_quarter,
                );
            });
    });
}

/// ZStack of background + label (+ optionally an occluder recorded
/// after the text, which must paint over it).
fn zorder_cell(ui: &mut Ui, label: &'static str, quad_after: bool) {
    captioned_cell(ui, label, |ui| {
        Panel::zstack()
            .id_salt((label, "box"))
            .size((Sizing::FILL, Sizing::FILL))
            .padding(12.0)
            .show(ui, |ui| {
                Frame::new()
                    .id_salt((label, "bg"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(swatch_bg(if quad_after { support::B } else { support::A }))
                    .show(ui);
                Text::new("T-shirt")
                    .id_salt((label, "label"))
                    .style(
                        TextStyle::default()
                            .with_font_size(28.0)
                            .with_color(Color::hex(0x1a1a1a)),
                    )
                    .show(ui);
                if quad_after {
                    Frame::new()
                        .id_salt((label, "occluder"))
                        .size((Sizing::Fixed(180.0), Sizing::Fixed(80.0)))
                        .background(swatch_bg(Color::hex(0x1a1a1a)))
                        .show(ui);
                }
            });
    });
}

/// Red field, centered blue card with a thick green stroke and 40 px
/// corners, black rect nested inside — its radius shrunk by the stroke
/// inset so the black corners follow the border's inner contour.
fn chrome_concentricity(ui: &mut Ui) {
    const STROKE: f32 = 8.0;
    const OUTER_CORNERS: f32 = 40.0;
    Panel::zstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .child_align(Align::CENTER)
        .background(Background {
            fill: Color::hex(0xff0000).into(),
            corners: Corners::all(4.0),
            ..Default::default()
        })
        .show(ui, |ui| {
            Panel::zstack()
                .auto_id()
                .size((Sizing::Fixed(200.0), Sizing::Fixed(120.0)))
                .child_align(Align::CENTER)
                .background(Background {
                    fill: Color::hex(0x0000ff).into(),
                    stroke: Stroke::solid(Color::hex(0x00ff00), STROKE),
                    corners: Corners::all(OUTER_CORNERS),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    Frame::new()
                        .auto_id()
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(Background {
                            fill: Color::hex(0x000000).into(),
                            corners: Corners::all(OUTER_CORNERS - STROKE - 1.0),
                            ..Default::default()
                        })
                        .show(ui);
                });
        });
}

/// Paint an opaque magenta backdrop so the next translucent draw
/// composites against a known non-black, non-white colour — making
/// the premultiplied-alpha bug obvious.
///
/// Backdrop = magenta `(1, 0, 1)`, translucent draw = green
/// `(0, 1, 0)` at α=0.5. Correct blend (premultiplied source):
/// `(0, 0.5, 0) + magenta * 0.5 = (0.5, 0.5, 0.5)` → mid grey.
/// Straight-alpha source into premul blend would give
/// `(0, 1, 0) + magenta * 0.5 = (0.5, 1, 0.5)` → bright green.
fn backdrop(ui: &mut Ui) {
    ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 120.0, 120.0)).fill(Color::rgb(1.0, 0.0, 1.0)));
}

/// Solid translucent polyline. Expected mid-grey diagonal.
fn translucent_solid(ui: &mut Ui) {
    backdrop(ui);
    let translucent_green = Color::rgba(0.0, 1.0, 0.0, 0.5);
    let pts = [Vec2::new(10.0, 20.0), Vec2::new(110.0, 100.0)];
    ui.add_shape(Shape::polyline(
        &pts,
        PolylineColors::Single(translucent_green),
        16.0,
    ));
}

/// Per-point translucent. Same expected muted mixes; the bug shows
/// as bright vertex colours.
fn translucent_per_point(ui: &mut Ui) {
    backdrop(ui);
    let pts = [
        Vec2::new(10.0, 20.0),
        Vec2::new(60.0, 100.0),
        Vec2::new(110.0, 20.0),
    ];
    let cols = [
        Color::rgba(1.0, 1.0, 0.0, 0.5),
        Color::rgba(0.0, 1.0, 1.0, 0.5),
        Color::rgba(1.0, 0.0, 1.0, 0.5),
    ];
    ui.add_shape(Shape::polyline(&pts, PolylineColors::PerPoint(&cols), 14.0));
}

/// α=0.25 — the bug grows linearly with `(1 - a)`, so a lower alpha
/// makes the over-bright effect even more obvious. Expected: the
/// magenta backdrop tinted slightly toward green.
fn translucent_quarter(ui: &mut Ui) {
    backdrop(ui);
    let pts = [Vec2::new(10.0, 60.0), Vec2::new(110.0, 60.0)];
    ui.add_shape(Shape::polyline(
        &pts,
        PolylineColors::Single(Color::rgba(0.0, 1.0, 0.0, 0.25)),
        24.0,
    ));
}
