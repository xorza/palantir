//! Motion over time: `Ui::animate` value interpolation (easing bars) and the drag lifecycle driving position (cards).

use crate::support;
use crate::support::{Section, api};
use palantir::glam::Vec2;
use palantir::widget::AnimationRequest;
use palantir::{
    AnimationSpec, Block, Button, Chrome, Configure, Corners, Easing, Panel, RgbaF32, Sense,
    Sizing, Stroke, Text, Ui, WidgetId,
};
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
