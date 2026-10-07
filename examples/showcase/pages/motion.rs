//! Motion over time: `Ui::animate` value interpolation (easing bars) and the drag lifecycle driving position (cards).

use crate::support;
use crate::support::{api, note, section};
use palantir::{
    AnimationSpec, Background, Block, Button, Configure, Corners, Easing, Panel, RgbaF32, Sense,
    Sizing, Stroke, Text, Ui, Vec2, WidgetId,
};
use std::time::Duration;

#[derive(Default, Debug)]
struct Bars {
    wide: bool,
}

const CANVAS_H: f32 = 300.0;
const CARD_W: f32 = 140.0;
const CARD_H: f32 = 80.0;

const CARDS: [(&str, Vec2, RgbaF32); 3] = [
    ("motion.card.a", Vec2::new(40.0, 32.0), support::A),
    ("motion.card.b", Vec2::new(230.0, 112.0), support::B),
    ("motion.card.c", Vec2::new(120.0, 192.0), support::D),
];

pub(crate) fn build(ui: &mut Ui) {
    easing(ui);
    drag(ui);
}

fn easing(ui: &mut Ui) {
    let demo_id = WidgetId::from_hash("motion::bars");
    section(
        ui,
        "Easing",
        &[
            api!(
                Ui::animate
                    as fn(&mut Ui, WidgetId, &'static str, f32, Option<AnimationSpec>) -> f32
            ),
            api!(AnimationSpec::duration),
        ],
        |ui| {
            note(
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
            for (key, label, spec) in [
                (
                    "linear-200",
                    "linear 200 ms",
                    AnimationSpec::duration(Duration::from_millis(200), Easing::Linear),
                ),
                (
                    "out-cubic-200",
                    "out-cubic 200 ms",
                    AnimationSpec::duration(Duration::from_millis(200), Easing::OutCubic),
                ),
                (
                    "out-back-300",
                    "out-back 300 ms — overshoots",
                    AnimationSpec::duration(Duration::from_millis(300), Easing::OutBack),
                ),
                ("spring-soft", "soft spring", AnimationSpec::SPRING),
            ] {
                bar(ui, key, label, spec, target);
            }
        },
    );
}

fn bar(
    ui: &mut Ui,
    key: &'static str,
    label: &'static str,
    spec: AnimationSpec,
    target_width: f32,
) {
    let id = WidgetId::from_hash(("motion::bar", key));
    let width = ui.animate(id, "width", target_width, Some(spec));
    Panel::hstack()
        .id_salt(key)
        .size((Sizing::FILL, Sizing::HUG))
        .gap(10.0)
        .show(ui, |ui| {
            Block::new()
                .id(id)
                .size((Sizing::fixed(width), Sizing::fixed(18.0)))
                .background(support::swatch_bg(support::A))
                .show(ui);
            Text::new(label).style(&support::note_style()).show(ui);
        });
}

/// Three draggable cards on a Canvas; `drag.delta()` applies to the position latched at drag start, and the dragged card records last to paint on top.
fn drag(ui: &mut Ui) {
    let dragging = CARDS.iter().position(|(k, _, _)| {
        ui.state::<CardState>(WidgetId::from_hash(*k))
            .is_some_and(|st| st.dragging)
    });

    section(ui, "Drag", &[], |ui| {
        note(
            ui,
            "Grab a card. Each card adds the drag delta to the position it had \
                 when the drag started, so the page tracks no pointer of its own. The \
                 card in hand records last, so it paints above the others.",
        );
        Panel::canvas()
            .size((Sizing::FILL, Sizing::fixed(CANVAS_H)))
            .background(support::well_bg())
            .clip_rounded()
            .show(ui, |ui| {
                for (i, (key, initial, accent)) in CARDS.iter().enumerate() {
                    if Some(i) != dragging {
                        card(ui, key, *initial, *accent);
                    }
                }
                if let Some(i) = dragging {
                    let (key, initial, accent) = CARDS[i];
                    card(ui, key, initial, accent);
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

fn card(ui: &mut Ui, key: &str, initial: Vec2, accent: RgbaF32) {
    let id = WidgetId::from_hash(key);
    ui.with_state::<CardState, _>(id, |ui, st| {
        let pos = st.pos.get_or_insert(initial);
        let r = Block::new()
            .id(id)
            .size((Sizing::fixed(CARD_W), Sizing::fixed(CARD_H)))
            .position(*pos)
            .sense(Sense::DRAG)
            .background(
                Background::rounded(accent, Corners::all(6.0))
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
