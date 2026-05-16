use crate::UiCore;
use crate::forest::element::Configure;
use crate::primitives::rect::Rect;
use crate::widgets::panel::Panel;
use crate::widgets::radio::RadioButton;
use glam::{UVec2, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pick {
    A,
    B,
    C,
}

/// Run one frame and return each row's reported `Response.rect`
/// keyed to the click target (B / C). We need the rect so the click
/// test can hit the actual painted area regardless of font metrics.
fn frame_with_rects(ui: &mut UiCore, surface: UVec2, sel: &mut Pick) -> [Option<Rect>; 3] {
    let mut local = *sel;
    let mut rects = [None; 3];
    ui.run_at(surface, |ui| {
        Panel::vstack().auto_id().gap(2.0).show(ui, |ui| {
            for (i, value) in [Pick::A, Pick::B, Pick::C].into_iter().enumerate() {
                let r = RadioButton::new(&mut local, value)
                    .id_salt(("rb", format!("{value:?}")))
                    .label(format!("{value:?}"))
                    .show(ui);
                rects[i] = r.rect();
            }
        });
    });
    *sel = local;
    rects
}

#[test]
fn clicking_a_row_selects_it() {
    let mut ui = UiCore::for_test();
    let surface = UVec2::new(300, 100);
    let mut sel = Pick::A;

    // First frame lays out (rects come back as None because the
    // response reads the *previous* frame's layout); ack, then a
    // second frame returns the first frame's rects.
    let _ = frame_with_rects(&mut ui, surface, &mut sel);
    let rects = frame_with_rects(&mut ui, surface, &mut sel);
    let row_b = rects[1].expect("row B rect");
    let row_c = rects[2].expect("row C rect");

    ui.click_at(row_b.min + (row_b.max() - row_b.min) * 0.5);
    frame_with_rects(&mut ui, surface, &mut sel);
    assert_eq!(sel, Pick::B, "click on row B selects B");

    ui.click_at(row_c.min + (row_c.max() - row_c.min) * 0.5);
    frame_with_rects(&mut ui, surface, &mut sel);
    assert_eq!(sel, Pick::C, "click on row C selects C");

    ui.click_at(row_c.min + (row_c.max() - row_c.min) * 0.5);
    frame_with_rects(&mut ui, surface, &mut sel);
    assert_eq!(sel, Pick::C, "re-click on selected row is no-op");
}

#[test]
fn disabled_radio_does_not_select() {
    let mut ui = UiCore::for_test();
    let surface = UVec2::new(300, 100);
    let mut sel = Pick::A;

    let mut local = sel;
    ui.run_at_acked(surface, |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            RadioButton::new(&mut local, Pick::B)
                .id_salt(("rb", "B"))
                .label("B")
                .disabled(true)
                .show(ui);
        });
    });
    sel = local;
    ui.click_at(Vec2::new(8.0, 8.0));
    let mut local = sel;
    ui.run_at(surface, |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            RadioButton::new(&mut local, Pick::B)
                .id_salt(("rb", "B"))
                .label("B")
                .disabled(true)
                .show(ui);
        });
    });
    sel = local;
    assert_eq!(sel, Pick::A, "disabled radio swallows click");
}
