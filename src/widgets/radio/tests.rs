use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::radio::RadioButton;
use glam::{UVec2, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pick {
    A,
    B,
    C,
}

/// One frame of the three rows, in row order.
///
/// The rects are the click targets: a test has to hit the actually
/// painted area, which font metrics decide.
struct Rows {
    rects: [Option<Rect>; 3],
    changed: [bool; 3],
}

/// `frame_value`, not `frame`: `changed` is a one-frame edge like
/// `clicked()`, so only the input-observing pass reports it.
fn frame_rows(h: &mut UiHarness, sel: &mut Pick) -> Rows {
    let mut local = *sel;
    let rows = h.frame_value(|ui| {
        let mut rows = Rows {
            rects: [None; 3],
            changed: [false; 3],
        };
        Panel::vstack().auto_id().gap(2.0).show(ui, |ui| {
            for (i, value) in [Pick::A, Pick::B, Pick::C].into_iter().enumerate() {
                let r = RadioButton::new(&mut local, value)
                    .id(WidgetId::from_hash(("rb", format!("{value:?}"))))
                    .label(format!("{value:?}"))
                    .show(ui);
                rows.rects[i] = r.response.rect;
                rows.changed[i] = r.changed;
            }
        });
        rows
    });
    *sel = local;
    rows
}

#[test]
fn clicking_a_row_selects_it() {
    let surface = UVec2::new(300, 100);
    let mut h = UiHarness::new(surface);
    let mut sel = Pick::A;

    // First frame lays out (rects come back as None because the
    // response reads the *previous* frame's layout); ack, then a
    // second frame returns the first frame's rects.
    let _ = frame_rows(&mut h, &mut sel);
    let rows = frame_rows(&mut h, &mut sel);
    let row_b = rows.rects[1].expect("row B rect");
    let row_c = rows.rects[2].expect("row C rect");
    assert_eq!(
        rows.changed, [false; 3],
        "an untouched frame reports no pick",
    );

    h.click_at(row_b.min + (row_b.max() - row_b.min) * 0.5);
    let rows = frame_rows(&mut h, &mut sel);
    assert_eq!(sel, Pick::B, "click on row B selects B");
    assert_eq!(
        rows.changed,
        [false, true, false],
        "only the row that took the pick reports `changed`",
    );

    h.click_at(row_c.min + (row_c.max() - row_c.min) * 0.5);
    let rows = frame_rows(&mut h, &mut sel);
    assert_eq!(sel, Pick::C, "click on row C selects C");
    assert_eq!(rows.changed, [false, false, true]);

    // The whole reason a radio hands back a `SelectResponse`: this frame
    // is `clicked()` on row C and `changed == false`, and `Response`
    // alone cannot tell the two apart.
    h.click_at(row_c.min + (row_c.max() - row_c.min) * 0.5);
    let rows = frame_rows(&mut h, &mut sel);
    assert_eq!(sel, Pick::C, "re-click on selected row is no-op");
    assert_eq!(
        rows.changed, [false; 3],
        "re-clicking the selected row reports no pick",
    );
}

#[test]
fn disabled_radio_does_not_select() {
    let surface = UVec2::new(300, 100);
    let mut h = UiHarness::new(surface);
    let mut sel = Pick::A;

    let mut local = sel;
    h.frame(|ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            RadioButton::new(&mut local, Pick::B)
                .id(WidgetId::from_hash(("rb", "B")))
                .label("B")
                .disabled(true)
                .show(ui);
        });
    });
    sel = local;
    h.click_at(Vec2::new(8.0, 8.0));
    let mut local = sel;
    h.frame(|ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            RadioButton::new(&mut local, Pick::B)
                .id(WidgetId::from_hash(("rb", "B")))
                .label("B")
                .disabled(true)
                .show(ui);
        });
    });
    sel = local;
    assert_eq!(sel, Pick::A, "disabled radio swallows click");
}
