//! Regression fixtures — deliberately colliding, occluding, or minimal
//! content that pins framework behavior by eye. Quarantined on their own
//! page so the intentionally ugly visuals don't leak into the widget
//! pages.
//!
//! - **id collisions**: siblings reuse one explicit `.id_salt(...)`; the
//!   framework disambiguates (state survives) and the always-on overlay
//!   paints a magenta 3 px outline over the offenders.
//! - **text z-order**: paint order is honored across quads and text —
//!   the composer splits draw groups on every text↔quad transition.
//! - **chrome concentricity**: rounded rect in rounded rect, the inner
//!   radius shrunk by the stroke inset so corners stay concentric.
//! - **premultiplied alpha**: translucent polylines over a magenta
//!   backdrop. Correct blending yields muted mixes; the historical
//!   straight-alpha-into-premul bug yields over-bright colors.

use crate::support;
use crate::support::{captioned_cell, demo_cell, section, swatch_bg, tiles};
use palantir::{
    Align, Background, Button, Color, Configure, Corners, Frame, Panel, PolylineColors, Rect,
    Shape, Sizing, Stroke, Text, TextStyle, Ui, Vec2,
};

pub(crate) fn build(ui: &mut Ui) {
    section(
        ui,
        "id collisions — the first row reuses one explicit id across siblings and \
         gets the magenta outline; the second row is clean",
        |ui| {
            support::row(ui, |ui| {
                for label in ["dup A", "dup B", "dup C"] {
                    Button::new().id_salt("idcol-dup-btn").label(label).show(ui);
                }
                for fill in [Color::hex(0x3a4a5c), Color::hex(0xddaa44)] {
                    Frame::new()
                        .id_salt("idcol-dup-frame")
                        .size(36.0)
                        .background(Background::fill(fill))
                        .show(ui);
                }
            });
            support::row(ui, |ui| {
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

    section(
        ui,
        "text z-order — record order decides who covers whom, quads and text alike",
        |ui| {
            tiles(ui, |ui| {
                zorder_cell(ui, "text over an earlier quad", false);
                zorder_cell(ui, "quad recorded AFTER the text covers it", true);
            });
        },
    );

    section(
        ui,
        "chrome & blending — concentric corners, and premultiplied-alpha repros \
         over a magenta backdrop",
        |ui| {
            tiles(ui, |ui| {
                captioned_cell(
                    ui,
                    "chrome concentricity",
                    support::TILE,
                    support::TILE,
                    concentricity,
                );
                demo_cell(ui, "premul — solid α 0.5, expect grey", translucent_solid);
                demo_cell(ui, "premul — per-point α 0.5", translucent_per_point);
                demo_cell(
                    ui,
                    "premul — α 0.25, expect slight tint",
                    translucent_quarter,
                );
            });
        },
    );
}

/// ZStack of background + label, optionally with an occluder recorded
/// after the text, which must paint over it.
#[track_caller]
fn zorder_cell(ui: &mut Ui, label: &'static str, quad_after: bool) {
    captioned_cell(ui, label, support::TILE, support::TILE, |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(12.0)
            .show(ui, |ui| {
                Frame::new()
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(swatch_bg(if quad_after { support::B } else { support::A }))
                    .show(ui);
                Text::new("T-shirt")
                    .style(
                        &TextStyle::default()
                            .with_font_size(28.0)
                            .with_color(Color::hex(0x14161a)),
                    )
                    .show(ui);
                if quad_after {
                    Frame::new()
                        .size((Sizing::fixed(120.0), Sizing::fixed(60.0)))
                        .background(swatch_bg(Color::hex(0x14161a)))
                        .show(ui);
                }
            });
    });
}

/// Red field, centered blue card with a thick green stroke and 40 px
/// corners, black rect nested inside — its radius shrunk by the stroke
/// inset so the black corners follow the border's inner contour.
fn concentricity(ui: &mut Ui) {
    const STROKE: f32 = 8.0;
    const OUTER: f32 = 40.0;
    Panel::zstack()
        .size((Sizing::FILL, Sizing::FILL))
        .child_align(Align::CENTER)
        .background(Background::rounded(Color::hex(0xff0000), Corners::all(4.0)))
        .show(ui, |ui| {
            Panel::zstack()
                .size((Sizing::fixed(130.0), Sizing::fixed(100.0)))
                .child_align(Align::CENTER)
                .background(
                    Background::rounded(Color::hex(0x0000ff), Corners::all(OUTER))
                        .with_stroke(Stroke::solid(Color::hex(0x00ff00), STROKE)),
                )
                .show(ui, |ui| {
                    Frame::new()
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(Background::rounded(
                            Color::hex(0x000000),
                            Corners::all(OUTER - STROKE - 1.0),
                        ))
                        .show(ui);
                });
        });
}

/// Paint an opaque magenta backdrop so the next translucent draw
/// composites against a known non-black, non-white colour — making the
/// premultiplied-alpha bug obvious.
///
/// Backdrop = magenta `(1, 0, 1)`, translucent draw = green `(0, 1, 0)`
/// at α=0.5. Correct blend (premultiplied source):
/// `(0, 0.5, 0) + magenta * 0.5 = (0.5, 0.5, 0.5)` → mid grey. A
/// straight-alpha source into a premul blend would give
/// `(0, 1, 0) + magenta * 0.5 = (0.5, 1, 0.5)` → bright green.
fn backdrop(ui: &mut Ui) {
    ui.add_shape(
        Shape::rect(Rect::new(0.0, 0.0, support::TILE, support::TILE))
            .fill(Color::rgb(1.0, 0.0, 1.0)),
    );
}

/// Solid translucent polyline. Expected mid-grey diagonal.
fn translucent_solid(ui: &mut Ui) {
    backdrop(ui);
    let pts = [Vec2::new(14.0, 28.0), Vec2::new(154.0, 140.0)];
    ui.add_shape(Shape::polyline(
        &pts,
        PolylineColors::Single(Color::rgba(0.0, 1.0, 0.0, 0.5)),
        16.0,
    ));
}

/// Per-point translucent. Same expected muted mixes; the bug shows as
/// bright vertex colours.
fn translucent_per_point(ui: &mut Ui) {
    backdrop(ui);
    let pts = [
        Vec2::new(14.0, 28.0),
        Vec2::new(84.0, 140.0),
        Vec2::new(154.0, 28.0),
    ];
    let cols = [
        Color::rgba(1.0, 1.0, 0.0, 0.5),
        Color::rgba(0.0, 1.0, 1.0, 0.5),
        Color::rgba(1.0, 0.0, 1.0, 0.5),
    ];
    ui.add_shape(Shape::polyline(&pts, PolylineColors::PerPoint(&cols), 14.0));
}

/// α=0.25 — the bug grows with `(1 - a)`, so a lower alpha makes the
/// over-bright effect even more obvious. Expected: the magenta backdrop
/// tinted slightly toward green.
fn translucent_quarter(ui: &mut Ui) {
    backdrop(ui);
    let pts = [Vec2::new(14.0, 84.0), Vec2::new(154.0, 84.0)];
    ui.add_shape(Shape::polyline(
        &pts,
        PolylineColors::Single(Color::rgba(0.0, 1.0, 0.0, 0.25)),
        24.0,
    ));
}
