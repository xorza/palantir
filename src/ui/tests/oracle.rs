//! A scene driven through one mutation per frame, with every retained
//! result checked against a cold one after each frame — see
//! [`Oracle`](crate::internals::harness::oracle::Oracle).
//!
//! One test per mutation, so a known gap is one `#[ignore]` naming the
//! redesign step that closes it rather than a hole in a shared script.

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::internals::harness::oracle::Oracle;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::{Image, ImageFit};
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::image_registry::image_handle::ImageHandle;
use crate::scene::layer::Layer;
use crate::scene::tree::paint_anims::curves;
use crate::scene::tree::paint_anims::paint_anim::{PaintAnim, PaintRepeat};
use crate::shape::Shape;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel, text::Text};
use glam::{UVec2, Vec2};
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(480, 360);

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

fn fill(c: RgbaF32) -> Background {
    Background::fill(c)
}

fn scene(ui: &mut Ui, k: Knobs, picture: &ImageHandle) {
    Panel::vstack()
        .id(WidgetId::from_hash("root"))
        .size((Sizing::FILL, Sizing::FILL))
        .gap(4.0)
        .show(ui, |ui| {
            Block::new()
                .id(WidgetId::from_hash("swatch"))
                .size(40.0)
                .background(fill(k.fill))
                .show(ui);
            Text::new(k.label).id(WidgetId::from_hash("label")).show(ui);
            Panel::canvas()
                .id(WidgetId::from_hash("canvas"))
                .size((Sizing::fixed(200.0), Sizing::fixed(40.0)))
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("mover"))
                        .position((k.nudge, 5.0))
                        .size(20.0)
                        .background(fill(RgbaF32::srgb(0.9, 0.2, 0.2)))
                        .show(ui);
                });
            Panel::zstack()
                .id(WidgetId::from_hash("spinner"))
                .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                .show(ui, |ui| {
                    let line = Shape::line(
                        Vec2::new(10.0, 20.0),
                        Vec2::new(70.0, 20.0),
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
                .id(WidgetId::from_hash("picture"))
                .size(40.0)
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
                    .id(WidgetId::from_hash("extra"))
                    .size(30.0)
                    .background(fill(RgbaF32::srgb(0.3, 0.8, 0.3)))
                    .show(ui);
            }
            let mut maybe = Block::new()
                .id(WidgetId::from_hash("maybe-hidden"))
                .size(30.0)
                .background(fill(RgbaF32::srgb(0.8, 0.8, 0.3)));
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
            .fixed_at(Vec2::new(300.0, 40.0))
            .show(|ui| {
                Block::new()
                    .id(WidgetId::from_hash(name))
                    .size(60.0)
                    .background(fill(color))
                    .show(ui);
            });
    }
}

/// Run `script`, one knob set per frame, with every oracle after each
/// frame. Two primed frames come first so the cascade and the damage
/// baseline are warm, and the mutation then lands on the incremental
/// paths it is meant to probe.
fn run(script: &[Knobs]) {
    let mut h = UiHarness::new(SURFACE);
    let picture = h
        .ui()
        .load_image(&Image::from_srgba8(
            UVec2::new(100, 100),
            vec![200; 100 * 100 * 4],
        ))
        .expect("a 100x100 image loads");
    let mut oracle = Oracle::default();
    for knobs in [Knobs::BASE, Knobs::BASE].iter().chain(script) {
        let report = h.frame(|ui| scene(ui, *knobs, &picture));
        oracle.check_frame(&h, &report);
    }
}

#[test]
fn colour_change() {
    run(&[Knobs {
        fill: RgbaF32::srgb(0.9, 0.9, 0.1),
        ..Knobs::BASE
    }]);
}

#[test]
fn text_change() {
    run(&[Knobs {
        label: "status: degraded since this morning",
        ..Knobs::BASE
    }]);
}

#[test]
fn move_in_canvas() {
    run(&[
        Knobs {
            nudge: 90.0,
            ..Knobs::BASE
        },
        Knobs {
            nudge: 30.0,
            ..Knobs::BASE
        },
    ]);
}

#[test]
fn add_and_remove_widget() {
    run(&[
        Knobs {
            extra: true,
            ..Knobs::BASE
        },
        Knobs::BASE,
    ]);
}

#[test]
fn hide_and_show() {
    run(&[
        Knobs {
            hidden: true,
            ..Knobs::BASE
        },
        Knobs::BASE,
    ]);
}

#[test]
fn shape_becomes_animated() {
    run(&[
        Knobs {
            spin: true,
            ..Knobs::BASE
        },
        Knobs::BASE,
    ]);
}

/// A 100 px image in a 40 px node switches to `ImageFit::None` and back:
/// its row grows past the node, and the damage covers the overflow both
/// ways.
#[test]
fn image_fit_overflows_its_node() {
    run(&[
        Knobs {
            fit_none: true,
            ..Knobs::BASE
        },
        Knobs::BASE,
    ]);
}

/// Content-equal roots swapping order change no row's `(hash, screen)`:
/// only the paint order of their overlap flips.
#[test]
fn roots_swap_order() {
    run(&[
        Knobs {
            swap_roots: true,
            ..Knobs::BASE
        },
        Knobs::BASE,
    ]);
}

#[test]
fn resize_between_frames() {
    let mut h = UiHarness::new(SURFACE);
    let picture = h
        .ui()
        .load_image(&Image::from_srgba8(
            UVec2::new(100, 100),
            vec![200; 100 * 100 * 4],
        ))
        .expect("a 100x100 image loads");
    let mut oracle = Oracle::default();
    for surface in [
        SURFACE,
        SURFACE,
        UVec2::new(300, 360),
        UVec2::new(301, 200),
        SURFACE,
    ] {
        h.resize(surface);
        let report = h.frame(|ui| scene(ui, Knobs::BASE, &picture));
        oracle.check_frame(&h, &report);
    }
}
