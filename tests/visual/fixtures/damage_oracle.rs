//! The pixel damage oracle: a frame repainted only where it was damaged
//! must equal the same frame painted in full, bit for bit.
//!
//! The CPU oracle (`internals::harness::oracle`) checks that damage covers every
//! paint row that changed. This one checks the pixels themselves, so it
//! also sees what no row describes. Each script runs through two renderers
//! side by side: one repaints only the damage, the other is told before
//! every frame that its target's contents are gone, so it paints
//! everything.

use std::time::Duration;

use glam::{UVec2, Vec2};
use palantir::widget::{PaintAnimation, PaintRepeat, Shape, curves};
use palantir::{
    Background, Block, Brush, Configure, Image, ImageFit, ImageHandle, Layer, Panel,
    RadialGradient, Rect, RgbaF32, Shadow, Sizing, Stroke, Text,
};

use crate::goldens::assert_same;
use crate::harness::Harness;

const SURFACE: UVec2 = UVec2::new(320, 240);

/// Everything the script can change, one field per mutation kind.
#[derive(Clone, Copy, Debug)]
struct Knobs {
    fill: RgbaF32,
    label: &'static str,
    spin: bool,
    swap_roots: bool,
    fit_none: bool,
    nudge: f32,
    extra: bool,
    hidden: bool,
    /// Small cards whose shadows share one cutout key (`shadows`).
    cards: usize,
}

impl Knobs {
    const BASE: Self = Self {
        fill: RgbaF32::srgb(0.2, 0.4, 0.8),
        label: "status: ok",
        spin: false,
        swap_roots: false,
        fit_none: false,
        nudge: 10.0,
        extra: false,
        hidden: false,
        cards: 4,
    };
}

fn scene(ui: &mut palantir::Ui, k: Knobs, picture: &ImageHandle) {
    // Translucent and varying in both axes, so every damaged pixel blends a
    // value of its own over the clear: a partial repaint over its pre-clear,
    // a full one over its `LoadOp::Clear`.
    let veil = RadialGradient::two_stop(
        RgbaF32::srgba(1.0, 1.0, 1.0, 0.6),
        RgbaF32::srgba(0.3, 0.6, 1.0, 0.05),
    );
    Panel::vstack()
        .id_salt("root")
        .size((Sizing::FILL, Sizing::FILL))
        .background(Background {
            fill: Brush::Radial(veil),
            ..Default::default()
        })
        .gap(4.0)
        .show(ui, |ui| {
            Block::new()
                .id_salt("swatch")
                .size(30.0)
                .background(Background::fill(k.fill))
                .show(ui);
            Text::new(k.label).id_salt("label").show(ui);
            Panel::canvas()
                .id_salt("canvas")
                .size((Sizing::fixed(200.0), Sizing::fixed(30.0)))
                .show(ui, |ui| {
                    Block::new()
                        .id_salt("mover")
                        .position((k.nudge, 5.0))
                        .size(20.0)
                        .background(Background::fill(RgbaF32::srgb(0.9, 0.2, 0.2)))
                        .show(ui);
                });
            Panel::zstack()
                .id_salt("spinner")
                .size((Sizing::fixed(80.0), Sizing::fixed(30.0)))
                .show(ui, |ui| {
                    let line = Shape::line(
                        Vec2::new(10.0, 15.0),
                        Vec2::new(70.0, 15.0),
                        Stroke::new(RgbaF32::WHITE, 2.0),
                    );
                    if k.spin {
                        ui.add_shape_animated(
                            line,
                            PaintAnimation::turn(0.0, 1.0)
                                .with_started_at(Duration::ZERO)
                                .with_period(Duration::from_secs(4))
                                .with_repeat(PaintRepeat::Forever)
                                .with_curve(curves::linear),
                        );
                    } else {
                        ui.add_shape(line);
                    }
                });
            Panel::zstack()
                .id_salt("picture")
                .size(30.0)
                .show(ui, |ui| {
                    let fit = if k.fit_none {
                        ImageFit::None
                    } else {
                        ImageFit::Fill
                    };
                    ui.add_shape(Shape::image(picture.clone()).fit(fit));
                });
            if k.extra {
                Block::new()
                    .id_salt("extra")
                    .size(20.0)
                    .background(Background::fill(RgbaF32::srgb(0.3, 0.8, 0.3)))
                    .show(ui);
            }
            let mut maybe = Block::new()
                .id_salt("maybe-hidden")
                .size(20.0)
                .background(Background::fill(RgbaF32::srgb(0.8, 0.8, 0.3)));
            if k.hidden {
                maybe = maybe.hidden();
            }
            maybe.show(ui);
        });
    let order = if k.swap_roots {
        ["front", "back"]
    } else {
        ["back", "front"]
    };
    for name in order {
        let color = if name == "front" {
            RgbaF32::srgb(1.0, 0.5, 0.0)
        } else {
            RgbaF32::srgb(0.0, 0.5, 1.0)
        };
        ui.layer(Layer::Popup)
            .fixed_at(Vec2::new(220.0, 30.0))
            .show(|ui| {
                Block::new()
                    .id_salt(name)
                    .size(60.0)
                    .background(Background::fill(color))
                    .show(ui);
            });
    }
    shadows(ui, k);
}

/// Shadows above the scene, each in a node of its own, so a partial frame
/// culls the ones its damage misses and plans them from the census. A large
/// one, σ = 16 and rounded 26, whose corners pay for their table alone. And
/// `cards` small ones along the top, σ = 4 and rounded 8, under the swatch
/// and the popups that other scripts damage: their 40×30 sources show about
/// 40.5² px of each corner's region, so a card's four corners cost 78.7k
/// nodes to shade against 259.6k to bake the `(8, 4)` table, which 4 cards
/// pay for and 3 do not. A frame that repaints one card reads the table
/// only through the census of the others, and one that removes or adds a
/// card leaves the rest unrepainted under a key that has just lost or
/// gained its table.
fn shadows(ui: &mut palantir::Ui, k: Knobs) {
    let shadow = |blur| Shadow {
        color: RgbaF32::srgba(0.1, 0.0, 0.2, 0.7),
        offset: Vec2::ZERO,
        blur,
        spread: 0.0,
        inset: false,
    };
    let cast = |ui: &mut palantir::Ui, id: usize, at: Rect, blur: f32, radius: f32| {
        Panel::canvas()
            .id_salt(("shadow", id))
            .position(at.min)
            .size((Sizing::fixed(at.size.w), Sizing::fixed(at.size.h)))
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::shadow(shadow(blur))
                        .at(Rect::new(0.0, 0.0, at.size.w, at.size.h))
                        .corners(radius),
                );
            });
    };
    ui.layer(Layer::Modal).fixed_at(Vec2::ZERO).show(|ui| {
        Panel::canvas()
            .id_salt("shadows")
            .size((Sizing::fixed(320.0), Sizing::fixed(240.0)))
            .show(ui, |ui| {
                cast(ui, 0, Rect::new(40.0, 60.0, 240.0, 150.0), 16.0, 26.0);
                for card in 0..k.cards {
                    let at = Rect::new(10.0 + 70.0 * card as f32, 16.0, 40.0, 30.0);
                    cast(ui, 1 + card, at, 4.0, 8.0);
                }
            });
    });
}

/// A 60 px checker, so an image drawn past its node is visible.
fn picture(h: &mut Harness) -> ImageHandle {
    let mut image = Image::from_srgba8(UVec2::splat(60), vec![0; 60 * 60 * 4]).unwrap();
    image.fill_with(|x, y| {
        let on = (x / 10 + y / 10) % 2 == 0;
        palantir::SrgbaU8::new(if on { 230 } else { 40 }, 120, 60, 255)
    });
    h.host.ui().load_image(&image).expect("a 60 px image loads")
}

/// Run `script` after two primed frames, comparing every frame.
fn run(name: &str, script: &[Knobs]) {
    let mut partial = Harness::new();
    let mut full = Harness::new();
    let (partial_picture, full_picture) = (picture(&mut partial), picture(&mut full));
    for (frame, k) in [Knobs::BASE, Knobs::BASE].iter().chain(script).enumerate() {
        let repainted = partial
            .size(SURFACE)
            .frame(|ui| scene(ui, *k, &partial_picture))
            .image;
        full.host.invalidate_target_contents();
        let painted = full
            .size(SURFACE)
            .frame(|ui| scene(ui, *k, &full_picture))
            .image;
        // Frame `n` is the script's entry `n - 2`, after the two base frames.
        assert_same(&format!("{name}_frame_{frame}"), &repainted, &painted);
    }
}

#[test]
fn colour_and_text_change() {
    run(
        "damage_oracle_colour_and_text_change",
        &[
            Knobs {
                fill: RgbaF32::srgb(0.9, 0.9, 0.1),
                ..Knobs::BASE
            },
            Knobs {
                label: "status: degraded since this morning",
                ..Knobs::BASE
            },
        ],
    );
}

#[test]
fn move_add_and_hide() {
    run(
        "damage_oracle_move_add_and_hide",
        &[
            Knobs {
                nudge: 90.0,
                ..Knobs::BASE
            },
            Knobs {
                extra: true,
                ..Knobs::BASE
            },
            Knobs {
                hidden: true,
                ..Knobs::BASE
            },
            Knobs::BASE,
        ],
    );
}

#[test]
fn image_overflow_and_root_swap() {
    run(
        "damage_oracle_image_overflow_and_root_swap",
        &[
            Knobs {
                fit_none: true,
                ..Knobs::BASE
            },
            Knobs::BASE,
            Knobs {
                swap_roots: true,
                ..Knobs::BASE
            },
            Knobs::BASE,
        ],
    );
}

#[test]
fn shape_becomes_animated() {
    run(
        "damage_oracle_shape_becomes_animated",
        &[
            Knobs {
                spin: true,
                ..Knobs::BASE
            },
            Knobs::BASE,
        ],
    );
}

#[test]
fn a_shadow_key_stops_and_starts_paying() {
    run(
        "damage_oracle_a_shadow_key_stops_and_starts_paying",
        &[
            Knobs {
                cards: 3,
                ..Knobs::BASE
            },
            Knobs {
                cards: 3,
                nudge: 90.0,
                ..Knobs::BASE
            },
            Knobs::BASE,
        ],
    );
}
