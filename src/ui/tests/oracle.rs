//! A scene driven through one mutation per frame, each retained result checked against a cold one ([`Oracle`](crate::internals::oracle::Oracle)); one test per mutation.

use crate::host::harness::Harness;
use crate::image_registry::image_handle::ImageHandle;
use crate::internals::oracle::Oracle;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::{Image, ImageFit};
use crate::primitives::paint::stroke::Stroke;
use crate::scene::layer::Layer;
use crate::shape::Shape;
use crate::shape::paint_animation::curves;
use crate::shape::paint_animation::{PaintAnimation, PaintRepeat};
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel, text::Text};
use glam::{UVec2, Vec2};
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(480, 360);

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
    /// `front` opens inside `back`'s body, one plane up, rather than beside it.
    nest_front: bool,
    /// Moves `front` right of `back`, so the two overlap less.
    front_shift: f32,
    /// A dialog holding a popup of its own, which records above the dialog.
    dialog: bool,
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
        nest_front: false,
        front_shift: 0.0,
        dialog: false,
    };
}

fn fill(c: RgbaF32) -> Chrome {
    Chrome::fill(c)
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
                .chrome(fill(k.fill))
                .show(ui);
            Text::new(k.label).id(WidgetId::from_hash("label")).show(ui);
            Panel::canvas()
                .id(WidgetId::from_hash("canvas"))
                .size((Sizing::fixed(200.0), Sizing::fixed(40.0)))
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("mover"))
                        .pos((k.nudge, 5.0))
                        .size(20.0)
                        .chrome(fill(RgbaF32::srgb(0.9, 0.2, 0.2)))
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
                    .chrome(fill(RgbaF32::srgb(0.3, 0.8, 0.3)))
                    .show(ui);
            }
            let mut maybe = Block::new()
                .id(WidgetId::from_hash("maybe-hidden"))
                .size(30.0)
                .chrome(fill(RgbaF32::srgb(0.8, 0.8, 0.3)));
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
    if k.nest_front {
        overlay(ui, "back", 0.0, |ui| {
            overlay(ui, "front", k.front_shift, |_| {});
        });
    } else {
        for name in order {
            let shift = if name == "front" { k.front_shift } else { 0.0 };
            overlay(ui, name, shift, |_| {});
        }
    }
    if k.dialog {
        ui.layer(Layer::Modal)
            .fixed_at(Vec2::new(40.0, 200.0))
            .show(|ui| {
                Block::new()
                    .id(WidgetId::from_hash("dialog"))
                    .size(80.0)
                    .chrome(fill(RgbaF32::srgb(0.4, 0.4, 0.4)))
                    .show(ui);
                ui.layer(Layer::Popup)
                    .fixed_at(Vec2::new(60.0, 220.0))
                    .show(|ui| {
                        Block::new()
                            .id(WidgetId::from_hash("dialog-popup"))
                            .size(50.0)
                            .chrome(fill(RgbaF32::srgb(0.9, 0.1, 0.6)))
                            .show(ui);
                    });
            });
    }
}

/// A 60 px popup block named `name` at (300 + `shift`, 40), with `body` recorded in its layer
/// scope after the block.
fn overlay(ui: &mut Ui, name: &'static str, shift: f32, body: impl FnOnce(&mut Ui)) {
    let color = if name == "front" {
        RgbaF32::srgb(1.0, 0.5, 0.0)
    } else {
        RgbaF32::srgb(0.0, 0.5, 1.0)
    };
    ui.layer(Layer::Popup)
        .fixed_at(Vec2::new(300.0 + shift, 40.0))
        .show(|ui| {
            Block::new()
                .id(WidgetId::from_hash(name))
                .size(60.0)
                .chrome(fill(color))
                .show(ui);
            body(ui);
        });
}

/// Run `script`, one knob set per frame, with every oracle after each; two primed frames warm the baselines.
fn run(script: &[Knobs]) {
    let mut h = Harness::mono(SURFACE);
    let picture = h
        .ui()
        .load_image(&Image::from_srgba8(UVec2::new(100, 100), vec![200; 100 * 100 * 4]).unwrap())
        .expect("a 100x100 image loads");
    let mut oracle = Oracle::default();
    for knobs in [Knobs::BASE, Knobs::BASE].iter().chain(script) {
        let report = h.frame(|ui| scene(ui, *knobs, &picture));
        oracle.check_frame(&h, &report);
    }
}

#[test]
fn color_change() {
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

/// A 100 px image in a 40 px node switches to `ImageFit::None` and back: damage covers the overflow both ways.
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
    let mut h = Harness::mono(SURFACE);
    let picture = h
        .ui()
        .load_image(&Image::from_srgba8(UVec2::new(100, 100), vec![200; 100 * 100 * 4]).unwrap())
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

/// `front` moves from beside `back` into its body, one plane up, and back out: same content and
/// rects, so only the plane change tells the retained results apart.
#[test]
fn an_overlay_nests_inside_another_and_back() {
    let nested = Knobs {
        nest_front: true,
        ..Knobs::BASE
    };
    run(&[nested, Knobs::BASE, nested]);
}

/// A nested overlay moves and stays nested: the move is confined to its plane.
#[test]
fn a_nested_overlay_moves() {
    let nested = Knobs {
        nest_front: true,
        ..Knobs::BASE
    };
    run(&[
        nested,
        Knobs {
            front_shift: 30.0,
            ..nested
        },
        nested,
    ]);
}

/// A dialog opens with a popup nested in it, which records on the plane above, and closes again.
#[test]
fn a_dialog_with_a_nested_popup_opens_and_closes() {
    run(&[
        Knobs {
            dialog: true,
            ..Knobs::BASE
        },
        Knobs::BASE,
        Knobs {
            dialog: true,
            ..Knobs::BASE
        },
    ]);
}

/// `front` paints under `back` as a sibling, then opens inside `back`, one plane up and over it:
/// the order of the overlap flips through the plane change alone, and the damage covers it.
#[test]
fn an_overlay_rises_a_plane_over_its_sibling() {
    run(&[
        Knobs {
            swap_roots: true,
            ..Knobs::BASE
        },
        Knobs {
            nest_front: true,
            ..Knobs::BASE
        },
        Knobs {
            swap_roots: true,
            ..Knobs::BASE
        },
    ]);
}
