use glam::Vec2;
use palantir::{
    Background, Color, Configure, Corners, Frame, Panel, Sense, Sizing, Stroke, Ui, WidgetId,
};

/// Three draggable cards on a Canvas. Each card stores its `Vec2`
/// in per-id state; `r.drag_position()` returns the anchored,
/// drag-delta-applied position with no caller-side anchor tracking.
/// The actively-dragged card is recorded last so it paints on top of
/// any overlap.
pub fn build(ui: &mut Ui) {
    let cards = [
        ("card.a", Vec2::new(40.0, 40.0), Color::hex(0x4d8eff)),
        ("card.b", Vec2::new(220.0, 120.0), Color::hex(0xff7a4d)),
        ("card.c", Vec2::new(120.0, 260.0), Color::hex(0x4dffa1)),
    ];

    let dragging_idx = cards
        .iter()
        .position(|(k, _, _)| ui.state_mut::<CardState>(WidgetId::from_hash(*k)).dragging);

    Panel::canvas()
        .id_salt("drag.canvas")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for (i, (key, initial, accent)) in cards.iter().enumerate() {
                if Some(i) == dragging_idx {
                    continue;
                }
                card(ui, key, *initial, *accent);
            }
            if let Some(i) = dragging_idx {
                let (key, initial, accent) = cards[i];
                card(ui, key, initial, accent);
            }
        });
}

const CARD_W: f32 = 140.0;
const CARD_H: f32 = 80.0;

#[derive(Default)]
struct CardState {
    pos: Vec2,
    /// Position at the moment `drag_started` fired; reused every
    /// subsequent frame as `pos = anchor + drag_delta`.
    anchor: Vec2,
    inited: bool,
    /// `true` between latch and release. Drives the "paint last" pick.
    dragging: bool,
}

fn card(ui: &mut Ui, key: &str, initial: Vec2, accent: Color) {
    let id = WidgetId::from_hash(key);
    let st: &mut CardState = ui.state_mut(id);
    if !st.inited {
        st.pos = initial;
        st.inited = true;
    }
    let pos = st.pos;

    let r = Frame::new()
        .id(id)
        .size((Sizing::Fixed(CARD_W), Sizing::Fixed(CARD_H)))
        .position(pos)
        .sense(Sense::DRAG)
        .background(Background {
            fill: accent.into(),
            stroke: Stroke::solid(Color::hex(0x202020), 1.0),
            radius: Corners::all(6.0),
        })
        .show(ui);

    let st: &mut CardState = ui.state_mut(id);
    if r.drag_started() {
        st.anchor = st.pos;
        st.dragging = true;
    }
    if let Some(delta) = r.drag_delta() {
        st.pos = st.anchor + delta;
    } else if st.dragging {
        st.dragging = false;
    }
}
