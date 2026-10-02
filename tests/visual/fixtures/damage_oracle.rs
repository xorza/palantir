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
use palantir::widget::{PaintAnim, PaintRepeat, Shape, curves};
use palantir::{
    Background, Block, Configure, Image, ImageFit, ImageHandle, Layer, Panel, RgbaF32, Sizing,
    Stroke, Text,
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
    };
}

fn scene(ui: &mut palantir::Ui, k: Knobs, picture: &ImageHandle) {
    Panel::vstack()
        .id_salt("root")
        .size((Sizing::FILL, Sizing::FILL))
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
                            PaintAnim::turn(0.0, 1.0)
                                .started_at(Duration::ZERO)
                                .period(Duration::from_secs(4))
                                .repeat(PaintRepeat::Forever)
                                .curve(curves::linear),
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
}

/// A 60 px checker, so an image drawn past its node is visible.
fn picture(h: &mut Harness) -> ImageHandle {
    let mut image = Image::from_srgba8(UVec2::splat(60), vec![0; 60 * 60 * 4]);
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
