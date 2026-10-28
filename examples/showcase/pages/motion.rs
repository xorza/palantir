//! Motion over time: `Ui::animate` value interpolation (easing bars), the drag lifecycle driving position (cards), and a paint animation's turn on every shape kind.

use crate::support;
use crate::support::DemoCell;
use crate::support::{Section, api};
use palantir::glam::{UVec2, Vec2};
use palantir::widget::{
    AnimationRequest, GlyphFont, IconFit, LineCap, Mesh, PaintAnimation, PaintRepeat, RectShape,
    Shape, curves,
};
use palantir::{
    AnimationSpec, Block, Button, Chrome, Configure, Corners, Easing, IconHandle, IconSet,
    IconTable, Image, ImageFit, ImageHandle, Panel, Rect, RgbaF32, Sense, Shadow, Sizing, Stroke,
    Text, Ui, WidgetId,
};
use std::rc::Rc;
use std::time::Duration;

#[derive(Default, Debug)]
struct Bars {
    wide: bool,
}

const CANVAS_H: f32 = 300.0;
const CARD_W: f32 = 140.0;
const CARD_H: f32 = 80.0;

const CARDS: [Card; 3] = [
    Card {
        key: "motion.card.a",
        initial: Vec2::new(40.0, 32.0),
        accent: support::A,
    },
    Card {
        key: "motion.card.b",
        initial: Vec2::new(230.0, 112.0),
        accent: support::B,
    },
    Card {
        key: "motion.card.c",
        initial: Vec2::new(120.0, 192.0),
        accent: support::D,
    },
];

pub(crate) fn build(ui: &mut Ui) {
    easing(ui);
    drag(ui);
    turn(ui);
}

const TURN_ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
<rect width="16" height="16" rx="3" fill="#2b3c5e"/><circle cx="5" cy="8" r="3" fill="#6ee7c8"/>
<rect x="9" y="4" width="4" height="8" rx="1" fill="#facc15"/>
</svg>"##;

/// What the turn demo draws from: an icon set and an image, loaded once and parked in a state row
/// so they stay loaded across frames.
#[derive(Clone, Debug)]
struct TurnSources {
    #[expect(dead_code, reason = "held so its icon stays loaded")]
    set: IconSet,
    icon: IconHandle,
    image: ImageHandle,
}

impl TurnSources {
    fn load(ui: &mut Ui) -> Self {
        ui.with_state::<Option<Self>, _>(
            WidgetId::from_hash("motion::turn::sources"),
            |ui, held| {
                held.get_or_insert_with(|| {
                    let set = ui.load_icons(Rc::new(
                        IconTable::from_svgs([("badge", TURN_ICON_SVG)])
                            .expect("the bundled icon is a readable SVG"),
                    ));
                    let texels = [
                        [230, 70, 60, 255],
                        [60, 160, 230, 255],
                        [250, 210, 60, 255],
                        [80, 200, 110, 255],
                    ];
                    let image = Image::from_srgba8(UVec2::new(2, 2), texels.concat())
                        .expect("a 2x2 image is four RGBA8 texels");
                    Self {
                        icon: set.by_name("badge").expect("bundled icon"),
                        image: ui
                            .load_image(&image)
                            .expect("a 2x2 image fits every supported GPU"),
                        set,
                    }
                })
                .clone()
            },
        )
    }
}

fn turn(ui: &mut Ui) {
    let sources = TurnSources::load(ui);
    Section::new("Turn")
        .apis(&[
            api!(Ui::add_shape_animated as fn(&mut Ui, RectShape, PaintAnimation)),
            api!(PaintAnimation::turn),
        ])
        .show(ui, |ui| {
            support::note(
                ui,
                "One paint animation turns each shape kind about its box's center, at paint \
                 time: the widget records the same shape every frame, and layout and hit tests \
                 never see the angle.",
            );
            let spin = PaintAnimation::turn(0.0, 1.0)
                .with_period(Duration::from_secs(6))
                .with_repeat(PaintRepeat::Forever)
                .with_curve(curves::linear);
            let source = Rect::new(20.0, 30.0, 60.0, 40.0);
            let mut mesh = Mesh::new();
            let a = mesh.vertex(Vec2::new(15.0, 80.0), RgbaF32::srgb(1.0, 0.3, 0.3));
            let b = mesh.vertex(Vec2::new(85.0, 80.0), RgbaF32::srgb(0.3, 1.0, 0.4));
            let c = mesh.vertex(Vec2::new(50.0, 15.0), RgbaF32::srgb(0.3, 0.5, 1.0));
            mesh.triangle(a, b, c);
            support::tiles(ui, |ui| {
                let cell = |ui: &mut Ui, label: &'static str, body: &mut dyn FnMut(&mut Ui)| {
                    DemoCell::new(label).size(120.0, 120.0).show(ui, |ui| {
                        Panel::zstack()
                            .id_salt(label)
                            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                            .show(ui, |ui| body(ui));
                    });
                };
                cell(ui, "rect", &mut |ui| {
                    ui.add_shape_animated(
                        Shape::rect(source)
                            .corners(Corners::all(10.0))
                            .fill(support::A)
                            .border(Stroke::new(RgbaF32::WHITE, 2.0)),
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
                        Shape::rect(source)
                            .corners(Corners::all(6.0))
                            .fill(support::B),
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
                        .fill(support::D),
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
                            Stroke::new(support::A, 30.0),
                        )
                        .cap(LineCap::Square),
                        spin,
                    );
                });
                cell(ui, "image", &mut |ui| {
                    ui.add_shape_animated(
                        Shape::image(sources.image.clone())
                            .at(source)
                            .fit(ImageFit::Fill),
                        spin,
                    );
                });
                cell(ui, "text", &mut |ui| {
                    let label = ui.intern("Turning");
                    ui.add_shape_animated(
                        Shape::text(label, GlyphFont::new(18.0))
                            .color(RgbaF32::WHITE)
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

fn easing(ui: &mut Ui) {
    let demo_id = WidgetId::from_hash("motion::bars");
    Section::new("Easing")
        .apis(&[
            api!(Ui::animate as fn(&mut Ui, AnimationRequest<f32>, Option<AnimationSpec>) -> f32),
            api!(AnimationSpec::duration),
        ])
        .show(ui, |ui| {
            support::note(
                ui,
                "Each bar retargets on the same frame through Ui::animate, with an \
                 AnimationSpec of its own. Click the button to send them all the \
                 other way.",
            );
            if Button::new().label("toggle").show(ui).clicked() {
                ui.with_state::<Bars, _>(demo_id, |_, s| s.wide = !s.wide);
            }
            let target = if ui.state::<Bars>(demo_id).is_some_and(|s| s.wide) {
                420.0
            } else {
                80.0
            };
            for bar in [
                Bar {
                    key: "linear-200",
                    label: "linear 200 ms",
                    spec: AnimationSpec::duration(Duration::from_millis(200), Easing::Linear),
                },
                Bar {
                    key: "out-cubic-200",
                    label: "out-cubic 200 ms",
                    spec: AnimationSpec::duration(Duration::from_millis(200), Easing::OutCubic),
                },
                Bar {
                    key: "out-back-300",
                    label: "out-back 300 ms — overshoots",
                    spec: AnimationSpec::duration(Duration::from_millis(300), Easing::OutBack),
                },
                Bar {
                    key: "spring-soft",
                    label: "soft spring",
                    spec: AnimationSpec::SPRING,
                },
            ] {
                bar.show(ui, target);
            }
        });
}

/// One easing bar: its width tweens to the target under its own spec.
#[derive(Debug)]
struct Bar {
    key: &'static str,
    label: &'static str,
    spec: AnimationSpec,
}

impl Bar {
    fn show(self, ui: &mut Ui, target_width: f32) {
        let id = WidgetId::from_hash(("motion::bar", self.key));
        let width = ui.animate(
            AnimationRequest::new(id, "width", target_width),
            Some(self.spec),
        );
        Panel::hstack()
            .id_salt(self.key)
            .size((Sizing::FILL, Sizing::HUG))
            .gap(10.0)
            .show(ui, |ui| {
                Block::new()
                    .id(id)
                    .size((Sizing::fixed(width), Sizing::fixed(18.0)))
                    .chrome(support::swatch_chrome(support::A))
                    .show(ui);
                Text::new(self.label).style(&support::note_style()).show(ui);
            });
    }
}

/// Three draggable cards on a Canvas; `drag.delta()` applies to the position latched at drag start, and the dragged card records last to paint on top.
fn drag(ui: &mut Ui) {
    let dragging = CARDS.iter().position(|card| {
        ui.state::<CardState>(WidgetId::from_hash(card.key))
            .is_some_and(|st| st.dragging)
    });

    Section::new("Drag").show(ui, |ui| {
        support::note(
            ui,
            "Grab a card. Each card adds the drag delta to the position it had \
                 when the drag started, so the page tracks no pointer of its own. The \
                 card in hand records last, so it paints above the others.",
        );
        Panel::canvas()
            .size((Sizing::FILL, Sizing::fixed(CANVAS_H)))
            .chrome(support::well_chrome())
            .clip_rounded()
            .show(ui, |ui| {
                for (i, card) in CARDS.iter().enumerate() {
                    if Some(i) != dragging {
                        card.show(ui);
                    }
                }
                if let Some(i) = dragging {
                    CARDS[i].show(ui);
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
    accent: RgbaF32,
}

impl Card {
    fn show(&self, ui: &mut Ui) {
        let id = WidgetId::from_hash(self.key);
        ui.with_state::<CardState, _>(id, |ui, st| {
            let pos = st.pos.get_or_insert(self.initial);
            let r = Block::new()
                .id(id)
                .size((Sizing::fixed(CARD_W), Sizing::fixed(CARD_H)))
                .pos(*pos)
                .sense(Sense::DRAG)
                .chrome(
                    Chrome::rounded(self.accent, Corners::all(6.0))
                        .with_border(Stroke::new(RgbaF32::hex(0x14161a), 1.0)),
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
