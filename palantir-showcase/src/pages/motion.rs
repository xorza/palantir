//! Motion over time: `Ui::animate` value interpolation (easing bars), the drag lifecycle driving position (cards), and a paint animation's turn on every shape kind.

use crate::made_image::MadeImage;
use crate::scaffold::api;
use crate::scaffold::tokens::M;
use crate::support;
use crate::support::DemoCell;
use crate::support::{Kit, Knob, Section, Swatches};
use palantir::glam::{UVec2, Vec2};
use palantir::widget::{
    AnimationRequest, GlyphFont, LineCap, LineJoin, Mesh, MotionRole, PaintAnimation, PaintRepeat,
    RectShape, Shape,
};
use palantir::{
    Align, Animated, AnimationSpec, Block, Border, Button, Chrome, Configure, Corners, Easing,
    IconFit, IconHandle, IconSet, IconTable, Image, ImageFit, Panel, Rect, RgbaF32, Sense, Shadow,
    Sizing, Spinner, Text, TranslateScale, Ui, VAlign, WidgetId,
};
use std::array;
use std::rc::Rc;
use std::time::Duration;

#[derive(Debug)]
struct Bars {
    wide: bool,
    /// The timed bars' duration in milliseconds, which the knob sets.
    duration_ms: f32,
    /// The last toggle's run, which the plots' dots ride.
    run: Option<Run>,
}

/// One toggle's run: when it started and the length it started under. A knob move reaches the next
/// run, as it reaches the bars, which keep the spec each run started under.
#[derive(Clone, Copy, Debug)]
struct Run {
    started: Duration,
    length: Duration,
}

impl Default for Bars {
    fn default() -> Self {
        Self {
            wide: false,
            duration_ms: 300.0,
            run: None,
        }
    }
}

/// Spinners, for work whose share is not known. Here and not on the Controls page, which a bench
/// holds still: a spinner wakes the host every frame.
fn spinner(ui: &mut Ui, k: Kit) {
    Section::new("Spinner")
        .apis(&[api!(Spinner::new)])
        .show(ui, k, |ui| {
            support::well(ui, k, |ui| {
                Panel::hstack()
                    .gap(M.space.lg)
                    .child_align(Align::v(VAlign::Center))
                    .show(ui, |ui| {
                        Spinner::new().size(20.0).show(ui);
                        Spinner::new().size(32.0).show(ui);
                        Spinner::new().size(48.0).color(k.swatch.orange).show(ui);
                    });
            });
        });
}

const CANVAS_H: f32 = 300.0;
const CARD_W: f32 = 140.0;
const CARD_H: f32 = 80.0;

const CARDS: [Card; 3] = [
    Card {
        key: "motion.card.a",
        initial: Vec2::new(40.0, 32.0),
        accent: |s| s.teal,
    },
    Card {
        key: "motion.card.b",
        initial: Vec2::new(230.0, 112.0),
        accent: |s| s.orange,
    },
    Card {
        key: "motion.card.c",
        initial: Vec2::new(120.0, 192.0),
        accent: |s| s.violet,
    },
];

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    spinner(ui, k);
    easing(ui, k);
    drag(ui, k);
    turn(ui, k);
}

const TURN_ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
<rect width="16" height="16" rx="3" fill="#2b3c5e"/><circle cx="5" cy="8" r="3" fill="#6ee7c8"/>
<rect x="9" y="4" width="4" height="8" rx="1" fill="#facc15"/>
</svg>"##;

fn turn_sources_id() -> WidgetId {
    WidgetId::from_hash("motion::turn::sources")
}

fn turn_image() -> Image {
    let texels = [
        [230, 70, 60, 255],
        [60, 160, 230, 255],
        [250, 210, 60, 255],
        [80, 200, 110, 255],
    ];
    Image::from_srgba8(UVec2::new(2, 2), texels.concat()).expect("a 2x2 image is four RGBA8 texels")
}

/// What the turn demo draws from: an icon set and an image, loaded once and parked in a state row
/// so they stay loaded across frames.
#[derive(Clone, Debug)]
struct TurnSources {
    #[expect(dead_code, reason = "held so its icon stays loaded")]
    set: IconSet,
    icon: IconHandle,
    image: MadeImage,
}

impl TurnSources {
    fn load(ui: &mut Ui) -> Self {
        ui.with_state::<Option<Self>, _>(turn_sources_id(), |ui, held| {
            held.get_or_insert_with(|| {
                let set = ui.load_icons(Rc::new(
                    IconTable::from_svgs([("badge", TURN_ICON_SVG)])
                        .expect("the bundled icon is a readable SVG"),
                ));
                Self {
                    icon: set.by_name("badge").expect("bundled icon"),
                    image: MadeImage::new(ui, turn_image),
                    set,
                }
            })
            .clone()
        })
    }
}

fn turn(ui: &mut Ui, k: Kit) {
    let sources = TurnSources::load(ui);
    Section::new("Turn")
        .apis(&[
            api!(Ui::add_shape_animated as fn(&mut Ui, RectShape, PaintAnimation)),
            api!(PaintAnimation::turn),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "One paint animation turns each shape kind about its box's center, at paint \
                 time: the widget records the same shape every frame, and layout and hit tests \
                 never see the angle.",
            );
            // The page shows motion, so its turn is essential and runs under reduced motion.
            let spin = PaintAnimation::turn(0.0, 1.0)
                .with_period(Duration::from_secs(6))
                .with_repeat(PaintRepeat::Forever)
                .with_motion(MotionRole::Essential);
            let source = Rect::new(20.0, 30.0, 60.0, 40.0);
            let mut mesh = Mesh::new();
            let a = mesh.vertex(Vec2::new(15.0, 80.0), RgbaF32::srgb(1.0, 0.3, 0.3));
            let b = mesh.vertex(Vec2::new(85.0, 80.0), RgbaF32::srgb(0.3, 1.0, 0.4));
            let c = mesh.vertex(Vec2::new(50.0, 15.0), RgbaF32::srgb(0.3, 0.5, 1.0));
            mesh.triangle(a, b, c);
            support::tiles(ui, |ui| {
                let cell = |ui: &mut Ui, label: &'static str, body: &mut dyn FnMut(&mut Ui)| {
                    DemoCell::new(label).size(120.0, 120.0).show(ui, k, |ui| {
                        Panel::zstack()
                            .id_salt(label)
                            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                            .show(ui, |ui| body(ui));
                    });
                };
                cell(ui, "rect", &mut |ui| {
                    ui.add_shape_animated(
                        Shape::rect(
                            Chrome::rounded(k.swatch.teal, Corners::all(10.0))
                                .with_border(Border::new(RgbaF32::WHITE, 2.0)),
                        )
                        .at(source),
                        spin,
                    );
                });
                cell(ui, "shadow", &mut |ui| {
                    ui.add_shape_animated(
                        Shape::shadow(Shadow::drop(RgbaF32::BLACK, Vec2::new(6.0, 6.0), 6.0))
                            .at(source)
                            .corners(Corners::all(6.0)),
                        spin,
                    );
                    ui.add_shape_animated(
                        Shape::rect(Chrome::rounded(k.swatch.orange, Corners::all(6.0))).at(source),
                        spin,
                    );
                });
                cell(ui, "triangle", &mut |ui| {
                    ui.add_shape_animated(
                        Shape::triangle(
                            Vec2::new(15.0, 80.0),
                            Vec2::new(85.0, 80.0),
                            Vec2::new(50.0, 15.0),
                        )
                        .radius(6.0_f32)
                        .fill(k.swatch.violet),
                        spin,
                    );
                });
                cell(ui, "mesh", &mut |ui| {
                    ui.add_shape_animated(Shape::mesh(&mesh), spin);
                });
                cell(ui, "dot", &mut |ui| {
                    ui.add_shape_animated(
                        Shape::line(
                            Vec2::splat(50.0),
                            Vec2::splat(50.0),
                            Border::new(k.swatch.teal, 30.0),
                        )
                        .cap(LineCap::Square),
                        spin,
                    );
                });
                cell(ui, "image", &mut |ui| {
                    ui.add_shape_animated(
                        Shape::image(sources.image.handle().clone())
                            .at(source)
                            .fit(ImageFit::Fill),
                        spin,
                    );
                });
                cell(ui, "text", &mut |ui| {
                    let label = ui.intern("Turning");
                    ui.add_shape_animated(
                        Shape::text(label, GlyphFont::new(18.0))
                            .color(k.t.ink)
                            .at_origin(Vec2::new(18.0, 40.0)),
                        spin,
                    );
                });
                cell(ui, "icon", &mut |ui| {
                    ui.add_shape_animated(
                        Shape::icon(sources.icon)
                            .at(Rect::new(25.0, 25.0, 50.0, 50.0))
                            .fit(IconFit::Fill),
                        spin,
                    );
                });
            });
        });
}

fn easing(ui: &mut Ui, k: Kit) {
    let demo_id = WidgetId::from_hash("motion::bars");
    Section::new("Easing")
        .apis(&[
            api!(Ui::animate as fn(&mut Ui, AnimationRequest<f32>, AnimationSpec) -> Animated<f32>),
            api!(AnimationSpec::duration),
            api!(
                AnimationRequest::with_motion
                    as fn(AnimationRequest<f32>, MotionRole) -> AnimationRequest<f32>
            ),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Each bar retargets on the same frame through Ui::animate, with an \
                 AnimationSpec of its own; the timed three share the knob's duration, which \
                 each run takes when it starts, the spring keeps its own. Toggle to send them \
                 all the other way. Each request's motion is essential, so the bars move under \
                 reduced motion too, where a decorative one snaps to its end.",
            );
            ui.with_state::<Bars, _>(demo_id, |ui, s| {
                support::row(ui, |ui| {
                    if Button::new().label("Toggle").show(ui).clicked() {
                        s.wide = !s.wide;
                        s.run = Some(Run {
                            started: ui.now(),
                            length: Duration::from_secs_f32(s.duration_ms / 1000.0),
                        });
                    }
                    Knob {
                        label: "duration",
                        range: 100.0..=2000.0,
                        step: 50.0,
                        unit: " ms",
                    }
                    .show(ui, k, &mut s.duration_ms);
                });
                let target = if s.wide { 420.0 } else { 80.0 };
                let duration = Duration::from_secs_f32(s.duration_ms / 1000.0);
                // The share of the run's length since the toggle, which each plot's dot sits at.
                let t = s.run.map_or(1.0, |run| {
                    (ui.now().saturating_sub(run.started).as_secs_f32() / run.length.as_secs_f32())
                        .min(1.0)
                });
                if t < 1.0 {
                    ui.request_repaint();
                }
                support::tiles(ui, |ui| {
                    for (label, easing) in [
                        ("linear", Easing::Linear),
                        ("out-cubic", Easing::OutCubic),
                        ("out-back", Easing::OutBack),
                    ] {
                        DemoCell::new(label).size(160.0, 120.0).show(ui, k, |ui| {
                            plot(ui, k, easing, t);
                        });
                    }
                });
                support::well(ui, k, |ui| {
                    for bar in [
                        Bar {
                            key: "linear",
                            label: "linear",
                            spec: AnimationSpec::duration(duration, Easing::Linear),
                        },
                        Bar {
                            key: "out-cubic",
                            label: "out-cubic",
                            spec: AnimationSpec::duration(duration, Easing::OutCubic),
                        },
                        Bar {
                            key: "out-back",
                            label: "out-back — overshoots",
                            spec: AnimationSpec::duration(duration, Easing::OutBack),
                        },
                        Bar {
                            key: "spring-soft",
                            label: "soft spring",
                            spec: AnimationSpec::SPRING,
                        },
                    ] {
                        bar.show(ui, k, target);
                    }
                });
            });
        });
}

/// The length of a bar's track: the 420 a bar heads for, and room for OutBack's overshoot.
const BAR_TRACK_W: f32 = 480.0;

/// One easing bar: its length tweens to the target under its own spec.
#[derive(Debug)]
struct Bar {
    key: &'static str,
    label: &'static str,
    spec: AnimationSpec,
}

impl Bar {
    fn show(self, ui: &mut Ui, k: Kit, target_width: f32) {
        let id = WidgetId::from_hash(("motion::bar", self.key));
        let width = ui
            .animate(
                AnimationRequest::new(id, "width", target_width).with_motion(MotionRole::Essential),
                self.spec,
            )
            .value;
        let bar = (Sizing::fixed(BAR_TRACK_W), Sizing::fixed(18.0));
        Panel::hstack()
            .id_salt(self.key)
            .size((Sizing::FILL, Sizing::HUG))
            .gap(M.space.md)
            .show(ui, |ui| {
                // A full bar slides out of a clip by a transform: a width is layout, which lands
                // on whole pixels and steps through a slow tail, where a transform moves the
                // snapped bar by sub-pixel steps.
                Panel::zstack()
                    .id(id.with("track"))
                    .size(bar)
                    .chrome(Chrome::rounded(
                        RgbaF32::TRANSPARENT,
                        Corners::all(M.radius.control),
                    ))
                    .clip_rounded()
                    .transform(TranslateScale::from_translation(Vec2::new(
                        width - BAR_TRACK_W,
                        0.0,
                    )))
                    .show(ui, |ui| {
                        Block::new()
                            .id(id)
                            .size(bar)
                            .chrome(support::swatch_chrome(k.swatch.teal))
                            .show(ui);
                    });
                Text::new(self.label).style(&k.t.caption()).show(ui);
            });
    }
}

/// Three draggable cards on a Canvas; `drag.delta()` applies to the position latched at drag start, and the dragged card records last to paint on top.
fn drag(ui: &mut Ui, k: Kit) {
    let dragging = CARDS.iter().position(|card| {
        ui.state::<CardState>(WidgetId::from_hash(card.key))
            .is_some_and(|st| st.dragging)
    });

    Section::new("Drag").show(ui, k, |ui| {
        support::note(
            ui,
            k,
            "Grab a card. Each card adds the drag delta to the position it had \
                 when the drag started, so the page tracks no pointer of its own. The \
                 card in hand records last, so it paints above the others.",
        );
        Panel::canvas()
            .size((Sizing::FILL, Sizing::fixed(CANVAS_H)))
            .chrome(k.t.card())
            .clip_rounded()
            .show(ui, |ui| {
                for (i, card) in CARDS.iter().enumerate() {
                    if Some(i) != dragging {
                        card.show(ui, k);
                    }
                }
                if let Some(i) = dragging {
                    CARDS[i].show(ui, k);
                }
            });
    });
}

#[derive(Default, Debug)]
struct CardState {
    pos: Option<Vec2>,
    /// Position when `drag_started` fired; `pos = anchor + drag_delta`.
    anchor: Vec2,
    dragging: bool,
}

/// A draggable card, keyed by `key` and placed at `initial` until first dragged.
#[derive(Debug)]
struct Card {
    key: &'static str,
    initial: Vec2,
    /// The card's fill, of the shown scheme's swatches.
    accent: fn(&Swatches) -> RgbaF32,
}

impl Card {
    fn show(&self, ui: &mut Ui, k: Kit) {
        let id = WidgetId::from_hash(self.key);
        ui.with_state::<CardState, _>(id, |ui, st| {
            let pos = st.pos.get_or_insert(self.initial);
            let r = Block::new()
                .id(id)
                .size((Sizing::fixed(CARD_W), Sizing::fixed(CARD_H)))
                .at(*pos)
                .sense(Sense::DRAG)
                .chrome(
                    Chrome::rounded((self.accent)(&k.swatch), Corners::all(6.0))
                        .with_border(Border::new(k.t.border, 1.0)),
                )
                .show(ui);
            if r.left.drag.started() {
                st.anchor = *pos;
                st.dragging = true;
            }
            if let Some(delta) = r.left.drag.delta() {
                *pos = st.anchor + delta;
            } else if st.dragging {
                st.dragging = false;
            }
        });
    }
}

/// The plot's frame inside its cell: room above the top for out-back's overshoot.
const PLOT: Rect = Rect::new(8.0, 24.0, 128.0, 72.0);
/// Points along a plotted curve, enough that a curve's polyline reads smooth at the plot's size.
const PLOT_POINTS: usize = 33;

/// `easing` as a curve, time across and value up, and a dot at `t` riding it.
fn plot(ui: &mut Ui, k: Kit, easing: Easing, t: f32) {
    let at = |t: f32| {
        Vec2::new(
            PLOT.min.x + t * PLOT.size.w,
            PLOT.min.y + (1.0 - easing.apply(t)) * PLOT.size.h,
        )
    };
    let points: [Vec2; PLOT_POINTS] = array::from_fn(|i| at(i as f32 / (PLOT_POINTS - 1) as f32));
    ui.add_shape(Shape::polyline(&points, Border::new(k.t.ink_dim, 1.5)).join(LineJoin::Round));
    let dot = at(t);
    ui.add_shape(
        Shape::rect(Chrome::rounded(k.swatch.orange, Corners::all(5.0))).at(Rect::new(
            dot.x - 5.0,
            dot.y - 5.0,
            10.0,
            10.0,
        )),
    );
}

#[cfg(test)]
mod tests {
    use palantir::glam::UVec2;
    use palantir::internals::headless_test_gpu;
    use palantir::{ColorScheme, Depth, Harness, Palette, Ui};

    use crate::pages::motion::{TurnSources, turn, turn_sources_id};
    use crate::support::Kits;

    /// A new device holds none of the turn demo's image, and the section's next frame fills it
    /// again: the window paints as it did on the old device.
    #[test]
    fn a_new_device_gets_the_turn_image_again() {
        let gpu = headless_test_gpu();
        let k = Kits::default().kit(ColorScheme::Dark, &Palette::DARK);
        let mut record = |ui: &mut Ui| turn(ui, k);
        let mut h = Harness::builder(UVec2::new(1000, 600))
            .depth(Depth::gpu(gpu.handles()))
            .build();
        h.frame(&mut record);
        let before = h.read_back();
        let image = h
            .state::<Option<TurnSources>>(turn_sources_id())
            .unwrap()
            .as_ref()
            .unwrap()
            .image
            .handle()
            .clone();

        h.recover(gpu.handles());
        assert!(!image.is_resident());
        h.frame(&mut record);
        assert!(image.is_resident());
        assert!(
            h.read_back() == before,
            "the section paints as before the new device"
        );
    }
}
