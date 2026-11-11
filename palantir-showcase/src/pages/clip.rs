//! Clip modes and subtree transforms. Each card holds a child overflowing on all four sides via negative margins; the clip mode decides what survives (none spills, rect cuts square, rounded trims to the corner radius), and padding moves the boundary to the content rect. `TranslateScale` applies to whole subtrees, stroke widths included.

use crate::scaffold::api;
use crate::support;
use crate::support::{DemoCell, Kit, Knob, Section, Surface};
use palantir::glam::Vec2;
use palantir::widget::{CubicBezier, QuadraticBezier, Shape};
use palantir::{
    Align, Block, Border, Chrome, ComboBox, Configure, Corners, LineCap, LinearGradient, Paint,
    Panel, Rect, RgbaF32, Sizing, Stroke, Text, TranslateScale, Ui, WidgetId,
};

const CARD: f32 = 200.0;
/// How far the child overhangs the card on every side; the cell adds it to all four, so "no clip" spills into empty space, not the neighboring tile.
const SPILL: f32 = 18.0;
const CELL: f32 = CARD + 2.0 * SPILL;

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    live(ui, k);
    Section::new("Clip modes")
        .apis(&[api!(Panel::clip_rect), api!(Panel::clip_rounded)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "The same child, overflowing the card by 18 px on every side, under each mode.",
            );
            support::tiles(ui, |ui| {
                for (label, mode) in [
                    ("None — the child spills", Mode::None),
                    ("Rect — square cut at the bounds", Mode::Rect),
                    ("Rounded — follows the radius", Mode::Rounded),
                ] {
                    ClipCard {
                        label,
                        mode,
                        padding: 0.0,
                    }
                    .show(ui, k);
                }
            });
        });

    Section::new("Clip and padding").show(ui, k, |ui| {
        support::note(
            ui,
            k,
            "Padding moves the clip boundary in, to the content rect, and the rounded mask \
                 follows the same edge.",
        );
        support::tiles(ui, |ui| {
            for card in [
                ClipCard {
                    label: "padded, no clip",
                    mode: Mode::None,
                    padding: 28.0,
                },
                ClipCard {
                    label: "padded, Rect",
                    mode: Mode::Rect,
                    padding: 28.0,
                },
                ClipCard {
                    label: "padded, Rounded",
                    mode: Mode::Rounded,
                    padding: 14.0,
                },
            ] {
                card.show(ui, k);
            }
        });
    });

    cases(ui, k);

    Section::new("Subtree transform")
        .apis(&[api!(Panel::transform), api!(TranslateScale::from_scale)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A TranslateScale on a container moves and scales everything under it, stroke \
                 widths included. Nested transforms compose.",
            );
            let knobs = WidgetId::from_hash("showcase::clip::transform");
            ui.with_state::<TransformKnobs, _>(knobs, |ui, t| {
                for (label, value, range, step, unit) in [
                    ("move x", &mut t.dx, -40.0..=80.0, 2.0, " px"),
                    ("move y", &mut t.dy, -40.0..=80.0, 2.0, " px"),
                    ("scale", &mut t.scale, 0.5..=2.0, 0.05, "×"),
                ] {
                    Knob {
                        label,
                        range,
                        step,
                        unit,
                    }
                    .show(ui, k, value);
                }
                let transform = TranslateScale::new(Vec2::new(t.dx, t.dy), t.scale);
                support::tiles(ui, |ui| {
                    DemoCell::new("the knobs' transform").show(ui, k, |ui| {
                        Panel::zstack()
                            .transform(transform)
                            .show(ui, |ui| tile(ui, k));
                    });
                });
            });
            support::tiles(ui, |ui| {
                DemoCell::new("translate (30, 24)").show(ui, k, |ui| {
                    Panel::zstack()
                        .transform(TranslateScale::from_translation(Vec2::new(30.0, 24.0)))
                        .show(ui, |ui| tile(ui, k));
                });
                DemoCell::new("scale 1.5 — strokes scale too").show(ui, k, |ui| {
                    Panel::zstack()
                        .transform(TranslateScale::from_scale(1.5))
                        .show(ui, |ui| tile(ui, k));
                });
                DemoCell::new("composed — scale 1.25, then translate").show(ui, k, |ui| {
                    Panel::zstack()
                        .transform(TranslateScale::from_scale(1.25))
                        .show(ui, |ui| {
                            Panel::zstack()
                                .transform(TranslateScale::from_translation(Vec2::new(20.0, 10.0)))
                                .show(ui, |ui| tile(ui, k));
                        });
                });
            });
        });
}

/// The board each rounded-clip case draws on, a canvas its clips sit in at fixed points.
const BOARD: f32 = 200.0;

/// Rounded clips against what crosses them: curves, text, shapes, strokes and other clips, each
/// placed so part of it falls outside. Every edge it crosses should be the clip's own arc,
/// antialiased, with nothing past it.
fn cases(ui: &mut Ui, k: Kit) {
    Section::new("Rounded clip cases")
        .apis(&[api!(Panel::clip_rounded), api!(Corners::new)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Each clip is a rounded rect, a circle at half its side; what it holds runs past \
                 its edge, and nested clips multiply, so an inner clip that hangs out of its \
                 parent is cut by both.",
            );
            support::tiles(ui, |ui| {
                board(ui, k, "circle — a curve leaves it", curve_in_circle);
                board(ui, k, "circle in an offset circle", circle_in_circle);
                board(ui, k, "four radii — 56 / 6 / 36 / 0", four_radii);
                board(ui, k, "pill — text runs past", text_in_pill);
                board(ui, k, "three nested, each offset", three_nested);
                board(ui, k, "child fills its clip exactly", filled_clip);
                board(ui, k, "hairlines across a corner", hairlines);
                board(ui, k, "shapes poke out of a circle", shapes_in_circle);
                board(ui, k, "a scaled subtree in a circle", scaled_in_circle);
                board(ui, k, "inside a 6 px border", inside_border);
            });
        });
}

/// One case on its own canvas, [`BOARD`] square.
fn board(ui: &mut Ui, k: Kit, label: &'static str, body: fn(&mut Ui, Kit)) {
    DemoCell::new(label)
        .size(BOARD, BOARD)
        .surface(Surface::Bare)
        .show(ui, k, |ui| {
            Panel::canvas()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| body(ui, k));
        });
}

/// A `w` × `h` rounded clip at `pos` in its parent canvas, framed so its edge shows, holding
/// `body` in a canvas of its own.
fn clip_at(
    ui: &mut Ui,
    k: Kit,
    pos: Vec2,
    size: Vec2,
    corners: Corners,
    body: impl FnOnce(&mut Ui),
) {
    Panel::canvas()
        .at(pos)
        .size((Sizing::fixed(size.x), Sizing::fixed(size.y)))
        .chrome(Chrome::rounded(k.t.ground, corners).with_border(Border::new(k.t.border, 1.0)))
        .clip_rounded()
        .show(ui, body);
}

/// A canvas child that fills its clip, so shapes added in `body` are laid out in the clip's rect.
fn layer(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::zstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, body);
}

/// A layer filled with `color` edge to edge, which shows the clips over it.
fn wash(ui: &mut Ui, color: RgbaF32) {
    layer(ui, |ui| {
        ui.add_shape(Shape::rect(), color);
    });
}

/// A circle of diameter `d` at `pos`.
fn circle_at(ui: &mut Ui, k: Kit, pos: Vec2, d: f32, body: impl FnOnce(&mut Ui)) {
    clip_at(ui, k, pos, Vec2::splat(d), Corners::all(d / 2.0), body);
}

fn curve_in_circle(ui: &mut Ui, k: Kit) {
    circle_at(ui, k, Vec2::splat(25.0), 150.0, |ui| {
        layer(ui, |ui| {
            ui.add_shape(
                Shape::cubic_bezier(CubicBezier {
                    p0: Vec2::new(-40.0, 120.0),
                    p1: Vec2::new(30.0, -60.0),
                    p2: Vec2::new(110.0, 210.0),
                    p3: Vec2::new(190.0, 30.0),
                }),
                Stroke::new(k.swatch.orange, 14.0).with_cap(LineCap::Round),
            );
            ui.add_shape(
                Shape::quadratic_bezier(QuadraticBezier {
                    p0: Vec2::new(-20.0, -20.0),
                    p1: Vec2::new(150.0, 40.0),
                    p2: Vec2::new(60.0, 180.0),
                }),
                Stroke::new(k.swatch.teal, 4.0),
            );
        });
    });
}

fn circle_in_circle(ui: &mut Ui, k: Kit) {
    circle_at(ui, k, Vec2::new(10.0, 25.0), 150.0, |ui| {
        wash(ui, k.swatch.violet.with_alpha(0.35));
        circle_at(ui, k, Vec2::new(70.0, 45.0), 110.0, |ui| {
            layer(ui, |ui| {
                ui.add_shape(Shape::rect(), k.swatch.lime);
                for i in 0..6 {
                    let x = i as f32 * 20.0 - 4.0;
                    ui.add_shape(
                        Shape::rect().at(Rect::new(x, -10.0, 8.0, 130.0)),
                        k.swatch.teal,
                    );
                }
            });
        });
    });
}

fn four_radii(ui: &mut Ui, k: Kit) {
    let size = Vec2::new(170.0, 140.0);
    clip_at(
        ui,
        k,
        Vec2::new(15.0, 30.0),
        size,
        Corners::new(56.0, 6.0, 36.0, 0.0),
        |ui| {
            layer(ui, |ui| {
                ui.add_shape(
                    Shape::rect().at(Rect::new(-20.0, -20.0, size.x + 40.0, size.y + 40.0)),
                    LinearGradient::new(0.6, [k.swatch.red, k.swatch.teal]),
                );
                for i in 0..8 {
                    let x = i as f32 * 28.0 - 40.0;
                    ui.add_shape(
                        Shape::line(Vec2::new(x, -10.0), Vec2::new(x + 90.0, size.y + 10.0)),
                        Stroke::new(RgbaF32::WHITE, 2.0),
                    );
                }
            });
        },
    );
}

fn text_in_pill(ui: &mut Ui, k: Kit) {
    clip_at(
        ui,
        k,
        Vec2::new(10.0, 68.0),
        Vec2::new(180.0, 64.0),
        Corners::all(32.0),
        |ui| {
            Text::new("Overflowing text")
                .font_size(46.0)
                .color(k.swatch.orange)
                .at(Vec2::new(-14.0, -2.0))
                .show(ui);
        },
    );
}

fn three_nested(ui: &mut Ui, k: Kit) {
    clip_at(
        ui,
        k,
        Vec2::new(8.0, 8.0),
        Vec2::new(150.0, 150.0),
        Corners::all(30.0),
        |ui| {
            wash(ui, k.swatch.violet.with_alpha(0.3));
            circle_at(ui, k, Vec2::new(50.0, 50.0), 120.0, |ui| {
                wash(ui, k.swatch.teal.with_alpha(0.5));
                clip_at(
                    ui,
                    k,
                    Vec2::new(55.0, 20.0),
                    Vec2::new(90.0, 110.0),
                    Corners::all(12.0),
                    |ui| {
                        wash(ui, k.swatch.orange);
                    },
                );
            });
        },
    );
}

fn filled_clip(ui: &mut Ui, k: Kit) {
    circle_at(ui, k, Vec2::splat(30.0), 140.0, |ui| {
        Block::new()
            .size((Sizing::FILL, Sizing::FILL))
            .chrome(Chrome::new(k.swatch.teal))
            .show(ui);
    });
}

fn hairlines(ui: &mut Ui, k: Kit) {
    clip_at(
        ui,
        k,
        Vec2::new(30.0, 30.0),
        Vec2::new(150.0, 150.0),
        Corners::all(60.0),
        |ui| {
            layer(ui, |ui| {
                for i in 0..12 {
                    let t = i as f32 * 12.0;
                    ui.add_shape(
                        Shape::line(Vec2::new(-30.0, t - 20.0), Vec2::new(t + 10.0, -30.0)),
                        Stroke::new(RgbaF32::WHITE, 1.0),
                    );
                    ui.add_shape(
                        Shape::line(Vec2::new(t + 40.0, 180.0), Vec2::new(180.0, t + 40.0)),
                        Stroke::new(k.swatch.lime, 1.0),
                    );
                }
            });
        },
    );
}

fn shapes_in_circle(ui: &mut Ui, k: Kit) {
    circle_at(ui, k, Vec2::splat(30.0), 140.0, |ui| {
        layer(ui, |ui| {
            ui.add_shape(
                Shape::triangle(
                    Vec2::new(-30.0, 120.0),
                    Vec2::new(60.0, 10.0),
                    Vec2::new(90.0, 160.0),
                )
                .radius(10.0),
                Paint::from(k.swatch.violet).with_border(Border::new(RgbaF32::WHITE, 3.0)),
            );
            ui.add_shape(
                Shape::circle().at(Rect::new(80.0, -20.0, 90.0, 90.0)),
                Paint::from(k.swatch.orange).with_border(Border::new(RgbaF32::WHITE, 3.0)),
            );
            ui.add_shape(
                Shape::rect()
                    .at(Rect::new(95.0, 95.0, 70.0, 70.0))
                    .corners(Corners::all(14.0)),
                Border::new(k.swatch.teal, 5.0),
            );
        });
    });
}

fn scaled_in_circle(ui: &mut Ui, k: Kit) {
    circle_at(ui, k, Vec2::splat(30.0), 140.0, |ui| {
        Panel::canvas()
            .transform(TranslateScale::new(Vec2::new(-30.0, -30.0), 1.6))
            .show(ui, |ui| {
                layer(ui, |ui| {
                    for row in 0..5 {
                        for col in 0..5 {
                            let color = if (row + col) % 2 == 0 {
                                k.swatch.teal
                            } else {
                                k.swatch.orange
                            };
                            ui.add_shape(
                                Shape::rect()
                                    .at(Rect::new(col as f32 * 26.0, row as f32 * 26.0, 22.0, 22.0))
                                    .corners(Corners::all(4.0)),
                                color,
                            );
                        }
                    }
                });
            });
    });
}

fn inside_border(ui: &mut Ui, k: Kit) {
    Panel::zstack()
        .at(Vec2::splat(30.0))
        .size((Sizing::fixed(140.0), Sizing::fixed(140.0)))
        .chrome(
            Chrome::rounded(k.t.ground, Corners::all(40.0))
                .with_border(Border::new(k.swatch.violet, 6.0)),
        )
        .clip_rounded()
        .show(ui, spiller_of(k.swatch.lime));
}

/// A block overflowing its slot on every side by [`SPILL`], in `color`.
fn spiller_of(color: RgbaF32) -> impl FnOnce(&mut Ui) {
    move |ui| {
        Block::new()
            .size((Sizing::FILL, Sizing::FILL))
            .margin((-SPILL, -SPILL, -SPILL, -SPILL))
            .chrome(Chrome::new(color))
            .show(ui);
    }
}

/// The transform demo's knobs: a move and a scale.
#[derive(Debug)]
struct TransformKnobs {
    dx: f32,
    dy: f32,
    scale: f32,
}

impl Default for TransformKnobs {
    fn default() -> Self {
        Self {
            dx: 20.0,
            dy: 20.0,
            scale: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    None,
    Rect,
    Rounded,
}

/// Card with a large corner radius so the rect scissor and rounded clip differ clearly.
fn card_chrome(k: Kit) -> Chrome {
    Chrome::rounded(k.t.ground, Corners::all(28.0)).with_border(Border::new(k.t.border, 1.5))
}

/// A card under one clip mode, its child overflowing on every side.
#[derive(Debug)]
struct ClipCard {
    label: &'static str,
    mode: Mode,
    padding: f32,
}

impl ClipCard {
    fn show(self, ui: &mut Ui, k: Kit) {
        let Self {
            label,
            mode,
            padding,
        } = self;
        DemoCell::new(label)
            .size(CELL, CELL)
            .surface(Surface::Bare)
            .show(ui, k, |ui| {
                Panel::zstack()
                    .size((Sizing::FILL, Sizing::FILL))
                    .child_align(Align::CENTER)
                    .show(ui, |ui| {
                        let mut panel = Panel::zstack()
                            .size((Sizing::fixed(CARD), Sizing::fixed(CARD)))
                            .padding(padding)
                            .chrome(card_chrome(k));
                        panel = match mode {
                            Mode::None => panel,
                            Mode::Rect => panel.clip_rect(),
                            Mode::Rounded => panel.clip_rounded(),
                        };
                        panel.show(ui, |ui| spiller(ui, k));
                    });
            });
    }
}

/// Rectangle overflowing the card on all sides: the negative margin grows its slot past the content rect and `Fill` takes it all, so the overhang stays [`SPILL`] padded or not.
fn spiller(ui: &mut Ui, k: Kit) {
    Block::new()
        .size((Sizing::FILL, Sizing::FILL))
        .margin((-SPILL, -SPILL, -SPILL, -SPILL))
        // Translucent so the card edge stays visible beneath; unclipped would otherwise be a solid block.
        .chrome(Chrome::new(k.swatch.orange.with_alpha(0.8)))
        .show(ui);
}

#[track_caller]
fn tile(ui: &mut Ui, k: Kit) {
    Block::new()
        .auto_id()
        .size((Sizing::fixed(56.0), Sizing::fixed(56.0)))
        .chrome(support::swatch_chrome(k.swatch.teal))
        .show(ui);
}

static MODES: [(&str, Mode); 3] = [
    ("None", Mode::None),
    ("Rect", Mode::Rect),
    ("Rounded", Mode::Rounded),
];

/// The live card's knobs: its clip mode and its padding.
#[derive(Debug)]
struct LiveClip {
    mode: &'static (&'static str, Mode),
    padding: f32,
}

impl Default for LiveClip {
    fn default() -> Self {
        Self {
            mode: &MODES[2],
            padding: 0.0,
        }
    }
}

/// One card, its clip mode and its padding from the knobs, its child overflowing on every side.
fn live(ui: &mut Ui, k: Kit) {
    Section::new("Live clip")
        .apis(&[api!(Panel::clip_rounded)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Pick a clip mode and pad the card: the clip follows the content rect, and the \
                 rounded mask its corners.",
            );
            let id = WidgetId::from_hash("showcase::clip::live");
            ui.with_state::<LiveClip, _>(id, |ui, l| {
                support::row(ui, |ui| {
                    Text::new("clip").style(&k.t.caption()).show(ui);
                    ComboBox::labeled(&mut l.mode, &MODES, |mode| mode.0)
                        .size((Sizing::fixed(120.0), Sizing::HUG))
                        .show(ui);
                    Knob {
                        label: "padding",
                        range: 0.0..=48.0,
                        step: 2.0,
                        unit: " px",
                    }
                    .show(ui, k, &mut l.padding);
                });
                ClipCard {
                    label: "the knobs' card",
                    mode: l.mode.1,
                    padding: l.padding,
                }
                .show(ui, k);
            });
        });
}
