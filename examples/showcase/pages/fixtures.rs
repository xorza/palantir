//! Regression fixtures: deliberately colliding, occluding or minimal content that pins behavior by eye, apart so
//! the ugly visuals don't leak into widget pages: id collisions (the overlay outlines offenders), text z-order
//! (draw groups split on text↔quad transitions), chrome concentricity, and premultiplied-alpha blending.

use crate::support;
use crate::support::{DemoCell, Section, Surface, api};
use palantir::widget::Shape;
use palantir::{
    Align, Background, Block, Button, Configure, Corners, Panel, Rect, RgbaF32, Sizing, Stroke,
    Text, TextStyle, Ui, Vec2,
};

pub(crate) fn build(ui: &mut Ui) {
    Section::new("Id collisions")
        .apis(&[api!(
            Button::id_salt as fn(Button<'static>, &'static str) -> Button<'static>
        )])
        .show(ui, |ui| {
            support::note(
                ui,
                "The first row reuses one explicit id across siblings and gets the \
                 magenta outline. The second row is clean.",
            );
            support::row(ui, |ui| {
                for label in ["dup A", "dup B", "dup C"] {
                    Button::new().id_salt("idcol-dup-btn").label(label).show(ui);
                }
                for fill in [RgbaF32::hex(0x3a4a5c), RgbaF32::hex(0xddaa44)] {
                    Block::new()
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
        });

    Section::new("Text z-order").show(ui, |ui| {
        support::note(
            ui,
            "Record order decides what covers what, quads and text alike.",
        );
        support::tiles(ui, |ui| {
            zorder_cell(ui, "text over an earlier quad", Order::TextOver);
            zorder_cell(
                ui,
                "quad recorded AFTER the text covers it",
                Order::QuadOver,
            );
        });
    });

    Section::new("Chrome and blending").show(ui, |ui| {
        support::note(
            ui,
            "Concentric corners, and premultiplied-alpha repros over a magenta \
                 backdrop.",
        );
        support::tiles(ui, |ui| {
            DemoCell::new("chrome concentricity")
                .surface(Surface::Bare)
                .show(ui, concentricity);
            DemoCell::new("premul — solid α 0.5, expect gray").show(ui, translucent_solid);
            DemoCell::new("premul — per-point α 0.5").show(ui, translucent_per_point);
            DemoCell::new("premul — α 0.25, expect slight tint").show(ui, translucent_quarter);
        });
    });
}

/// Which of a z-order cell's text and second quad records last.
#[derive(Clone, Copy, Debug)]
enum Order {
    /// The text over the backdrop quad, nothing after it.
    TextOver,
    /// A dark quad recorded after the text, covering it.
    QuadOver,
}

fn zorder_cell(ui: &mut Ui, label: &'static str, order: Order) {
    DemoCell::new(label).surface(Surface::Bare).show(ui, |ui| {
        Panel::zstack()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(12.0)
            .show(ui, |ui| {
                Block::new()
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(support::swatch_bg(match order {
                        Order::TextOver => support::A,
                        Order::QuadOver => support::B,
                    }))
                    .show(ui);
                Text::new("T-shirt")
                    .style(
                        &TextStyle::default()
                            .with_font_size(28.0)
                            .with_color(RgbaF32::hex(0x14161a)),
                    )
                    .show(ui);
                if matches!(order, Order::QuadOver) {
                    Block::new()
                        .size((Sizing::fixed(120.0), Sizing::fixed(60.0)))
                        .background(support::swatch_bg(RgbaF32::hex(0x14161a)))
                        .show(ui);
                }
            });
    });
}

/// Red field, blue card with a thick green border and 40 px corners, black rect nested inside whose radius
/// shrinks by the border inset so its corners follow the border's inner contour.
fn concentricity(ui: &mut Ui) {
    const STROKE: f32 = 8.0;
    const OUTER: f32 = 40.0;
    Panel::zstack()
        .size((Sizing::FILL, Sizing::FILL))
        .child_align(Align::CENTER)
        .background(Background::rounded(
            RgbaF32::hex(0xff0000),
            Corners::all(4.0),
        ))
        .show(ui, |ui| {
            Panel::zstack()
                .size((Sizing::fixed(130.0), Sizing::fixed(100.0)))
                .child_align(Align::CENTER)
                .background(
                    Background::rounded(RgbaF32::hex(0x0000ff), Corners::all(OUTER))
                        .with_border(Stroke::new(RgbaF32::hex(0x00ff00), STROKE)),
                )
                .show(ui, |ui| {
                    Block::new()
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(Background::rounded(
                            RgbaF32::hex(0x000000),
                            Corners::all(OUTER - STROKE - 1.0),
                        ))
                        .show(ui);
                });
        });
}

/// Opaque magenta `(1, 0, 1)` backdrop so a translucent green `(0, 1, 0)` draw at α=0.5 shows a
/// premultiplied-alpha bug: correct blending gives mid gray `(0.5, 0.5, 0.5)`, straight alpha bright green `(0.5, 1, 0.5)`.
fn backdrop(ui: &mut Ui) {
    ui.add_shape(
        Shape::rect(Rect::new(0.0, 0.0, support::TILE, support::TILE))
            .fill(RgbaF32::srgb(1.0, 0.0, 1.0)),
    );
}

/// Solid translucent polyline. Expected mid-gray diagonal.
fn translucent_solid(ui: &mut Ui) {
    backdrop(ui);
    let pts = [Vec2::new(14.0, 28.0), Vec2::new(154.0, 140.0)];
    ui.add_shape(Shape::polyline(
        &pts,
        Stroke::new(RgbaF32::srgba(0.0, 1.0, 0.0, 0.5), 16.0),
    ));
}

/// Per-point translucent; same muted mixes, the bug shows as bright vertex colors.
fn translucent_per_point(ui: &mut Ui) {
    backdrop(ui);
    let pts = [
        Vec2::new(14.0, 28.0),
        Vec2::new(84.0, 140.0),
        Vec2::new(154.0, 28.0),
    ];
    let cols = [
        RgbaF32::srgba(1.0, 1.0, 0.0, 0.5),
        RgbaF32::srgba(0.0, 1.0, 1.0, 0.5),
        RgbaF32::srgba(1.0, 0.0, 1.0, 0.5),
    ];
    ui.add_shape(Shape::polyline(&pts, Stroke::new(RgbaF32::WHITE, 14.0)).per_point(&cols));
}

/// α=0.25: the bug grows with `(1 - a)`; expects the magenta backdrop tinted slightly toward green.
fn translucent_quarter(ui: &mut Ui) {
    backdrop(ui);
    let pts = [Vec2::new(14.0, 84.0), Vec2::new(154.0, 84.0)];
    ui.add_shape(Shape::polyline(
        &pts,
        Stroke::new(RgbaF32::srgba(0.0, 1.0, 0.0, 0.25), 24.0),
    ));
}
