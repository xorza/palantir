use super::*;

/// Caret blink: visible for the first half-period, hidden for the
/// second, repeats. Reset to "visible" by any caret / selection /
/// text change. Off entirely when the editor isn't focused.
#[test]
fn caret_blinks_on_and_off_while_focused() {
    use crate::forest::shapes::record::ShapeRecord;
    use crate::forest::tree::NodeId;
    use std::time::Duration;

    fn body(ui: &mut Ui, buf: &mut String, leaf: &mut Option<NodeId>) {
        Panel::hstack().auto_id().show(ui, |ui| {
            *leaf = Some(
                TextEdit::new(buf)
                    .id_salt("blink-ed")
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node,
            );
        });
    }

    fn caret_painted(ui: &Ui, leaf: NodeId) -> bool {
        // Caret is the only RoundedRect with `local_rect: Some(...)` on
        // a freshly focused, empty, unselected editor — `Background`
        // routes through `chrome` (no shape), selection wash is absent
        // without a selection.
        shapes_of(ui.forest.tree(Layer::Main), leaf).any(|s| {
            matches!(
                s,
                ShapeRecord::RoundedRect {
                    local_rect: Some(_),
                    ..
                }
            )
        })
    }

    fn frame_at(ui: &mut Ui, now_secs: f32, mut f: impl FnMut(&mut Ui)) {
        use crate::layout::types::display::Display;
        let display = Display::from_physical(NARROW, 1.0);
        ui.frame(display, Duration::from_secs_f32(now_secs), &mut (), |ui| {
            f(ui)
        });
        ui.frame_state.mark_submitted();
    }

    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::new();
    let mut leaf = None;

    // Frame 1: record editor unfocused.
    frame_at(&mut ui, 0.0, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        !caret_painted(&ui, leaf.unwrap()),
        "unfocused editor paints no caret",
    );

    // Click focuses; caret jumps to byte 0 (empty buf). Drive a fresh
    // frame at t=0 so handle_input drains the click. caret_changed =
    // true → last_caret_change = 0; elapsed = 0; phase 0; visible.
    click_at(&mut ui, Vec2::new(20.0, 20.0));
    frame_at(&mut ui, 0.0, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        caret_painted(&ui, leaf.unwrap()),
        "freshly focused: caret visible",
    );

    // Still inside the first half-period.
    frame_at(&mut ui, 0.3, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        caret_painted(&ui, leaf.unwrap()),
        "first half of blink cycle: caret visible",
    );

    // Crossed into the hidden half.
    frame_at(&mut ui, 0.7, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        !caret_painted(&ui, leaf.unwrap()),
        "second half of blink cycle: caret hidden",
    );

    // One full period later: visible again.
    frame_at(&mut ui, 1.2, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        caret_painted(&ui, leaf.unwrap()),
        "after a full period: caret visible again",
    );

    // Typing during a hidden phase must snap the caret back on.
    frame_at(&mut ui, 1.7, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        !caret_painted(&ui, leaf.unwrap()),
        "precondition: hidden phase before keystroke",
    );
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('a'),
        repeat: false,
    });
    frame_at(&mut ui, 1.75, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        caret_painted(&ui, leaf.unwrap()),
        "keystroke resets blink: caret immediately visible",
    );

    // Long idle: blink stops scheduling and caret stays visible so
    // an unattended focused editor doesn't keep the host repainting
    // at 2 Hz forever.
    frame_at(&mut ui, 100.0, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        caret_painted(&ui, leaf.unwrap()),
        "long-idle blink stops on the visible phase",
    );
}

/// Focused TextEdit must keep the host's repaint loop alive — without
/// the wake schedule, the blink would freeze on whichever phase the
/// last frame landed on.
#[test]
fn focused_text_edit_schedules_blink_wake() {
    use crate::layout::types::display::Display;
    use std::time::Duration;

    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::new();
    let display = Display::from_physical(NARROW, 1.0);

    // Unfocused: no blink schedule.
    let report = ui.frame(display, Duration::ZERO, &mut (), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(&mut buf)
                .id_salt("blink-wake")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    });
    assert_eq!(
        report.repaint_after(),
        None,
        "unfocused editor doesn't schedule blink wakes",
    );

    // Focus, then drive another frame — now the scheduler should
    // request a wake at the next phase boundary.
    click_at(&mut ui, Vec2::new(20.0, 20.0));
    let report = ui.frame(display, Duration::ZERO, &mut (), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(&mut buf)
                .id_salt("blink-wake")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    });
    assert!(
        report.repaint_after().is_some(),
        "focused editor schedules a blink wake",
    );
}
