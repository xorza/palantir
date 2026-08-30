//! Motion over time, from both ends of the API: `Ui::animate` driving
//! value interpolation (the easing bars), and the drag lifecycle on a
//! `Response` driving position directly (the cards).
//!
//! The bars double as the regression fixture for `Ui::animate`
//! end-to-end — target, tick, record, repaint. The cards show that
//! `drag.delta()` plus a latched anchor is the whole of drag handling:
//! no caller-side pointer tracking, no per-frame delta accumulation.

use crate::support;
use glam::Vec2;
use palantir::{
    AnimSpec, Background, Button, Color, Configure, Corners, Easing, Frame, Panel, Sense, Sizing,
    Stroke, Text, Ui, WidgetId,
};

#[derive(Default, Debug)]
struct Bars {
    wide: bool,
}

const CANVAS_H: f32 = 300.0;
const CARD_W: f32 = 140.0;
const CARD_H: f32 = 80.0;

const CARDS: [(&str, Vec2, Color); 3] = [
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
    support::section(
        ui,
        "easing — Ui::animate; every bar retargets at once, one AnimSpec each",
        |ui| {
            if Button::new()
                .id_salt("anim-go")
                .label("go")
                .show(ui)
                .left
                .clicked()
            {
                let s = ui.state_mut::<Bars>(demo_id);
                s.wide = !s.wide;
            }
            let target = if ui.state_mut::<Bars>(demo_id).wide {
                420.0
            } else {
                80.0
            };
            for (key, label, spec) in [
                (
                    "linear-200",
                    "linear 200 ms",
                    AnimSpec::duration(0.2, Easing::Linear),
                ),
                (
                    "out-cubic-200",
                    "out-cubic 200 ms",
                    AnimSpec::duration(0.2, Easing::OutCubic),
                ),
                (
                    "out-back-300",
                    "out-back 300 ms — overshoots",
                    AnimSpec::duration(0.3, Easing::OutBack),
                ),
                ("spring-soft", "soft spring", AnimSpec::SPRING),
            ] {
                bar(ui, key, label, spec, target);
            }
        },
    );
}

fn bar(ui: &mut Ui, key: &'static str, label: &'static str, spec: AnimSpec, target_width: f32) {
    let id = WidgetId::from_hash(("motion::bar", key));
    let width = ui.animate(id, "width", target_width, Some(spec));
    Panel::hstack()
        .id_salt(("anim-row", key))
        .size((Sizing::FILL, Sizing::HUG))
        .gap(10.0)
        .show(ui, |ui| {
            Frame::new()
                .id(id)
                .size((Sizing::fixed(width), Sizing::fixed(18.0)))
                .background(support::swatch_bg(support::A))
                .show(ui);
            Text::new(label)
                .id_salt(("anim-label", key))
                .style(&support::note_style())
                .show(ui);
        });
}

/// Three draggable cards on a Canvas. Each card stores its `Vec2` in
/// per-id state; `drag.delta()` is applied to the position latched when
/// the drag started, so no anchor bookkeeping leaks into the caller. The
/// actively-dragged card records last so it paints over any overlap.
fn drag(ui: &mut Ui) {
    let dragging = CARDS
        .iter()
        .position(|(k, _, _)| ui.state_mut::<CardState>(WidgetId::from_hash(*k)).dragging);

    support::section(
        ui,
        "drag — grab a card; the active one raises above its neighbors",
        |ui| {
            Panel::canvas()
                .id_salt("drag-canvas")
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
        },
    );
}

#[derive(Default, Debug)]
struct CardState {
    pos: Vec2,
    /// Position at the moment `drag_started` fired; reused every
    /// subsequent frame as `pos = anchor + drag_delta`.
    anchor: Vec2,
    /// `true` between latch and release. Drives the "record last" pick.
    dragging: bool,
}

fn card(ui: &mut Ui, key: &str, initial: Vec2, accent: Color) {
    let id = WidgetId::from_hash(key);
    // Seeded on the first frame only — keyed on the row not existing yet,
    // the way every other page seeds one, rather than on a flag the row's
    // own presence already answers.
    let fresh = ui.try_state::<CardState>(id).is_none();
    let st: &mut CardState = ui.state_mut(id);
    if fresh {
        st.pos = initial;
    }
    let pos = st.pos;

    let r = Frame::new()
        .id(id)
        .size((Sizing::fixed(CARD_W), Sizing::fixed(CARD_H)))
        .position(pos)
        .sense(Sense::DRAG)
        .background(
            Background::rounded(accent, Corners::all(6.0))
                .with_stroke(Stroke::solid(Color::hex(0x14161a), 1.0)),
        )
        .show(ui)
        .snapshot();

    let st: &mut CardState = ui.state_mut(id);
    if r.left.drag.started() {
        st.anchor = st.pos;
        st.dragging = true;
    }
    if let Some(delta) = r.left.drag.delta() {
        st.pos = st.anchor + delta;
    } else if st.dragging {
        st.dragging = false;
    }
}
