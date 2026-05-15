use super::*;
use crate::support::internals::ResponseNodeExt;

/// Caret blink: visible for the first half-period, hidden for the
/// second, repeats. Reset to "visible" by any caret / selection /
/// text change. Off entirely when the editor isn't focused.
#[test]
fn caret_blinks_on_and_off_while_focused() {
    use crate::forest::shapes::record::ShapeRecord;
    use crate::forest::tree::{NodeId, TreeItem, TreeItems};
    use std::time::Duration;

    fn body(ui: &mut Ui, buf: &mut String, leaf: &mut Option<NodeId>) {
        Panel::hstack().auto_id().show(ui, |ui| {
            *leaf = Some(
                TextEdit::new(buf)
                    .id_salt("blink-ed")
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node(ui),
            );
        });
    }

    fn caret_painted(ui: &Ui, leaf: NodeId) -> bool {
        // Caret is the only RoundedRect with `local_rect: Some(...)` on
        // a freshly focused, empty, unselected editor — `Background`
        // routes through `chrome` (no shape), selection wash is absent
        // without a selection. Post-`PaintAnim`-migration the rect is
        // always present when focused; the encoder hides it via the
        // attached `PaintAnim`. "Painted" now means "rect present AND
        // its anim (if any) samples to visible at the current time".
        let tree = ui.forest.tree(Layer::Main);
        let now = ui.time;
        TreeItems::new(&tree.records, &tree.shapes.records, leaf)
            .filter_map(|item| match item {
                TreeItem::ShapeRecord(idx, s) => Some((idx, s)),
                TreeItem::Child(_) => None,
            })
            .any(|(idx, s)| {
                let is_caret = matches!(
                    s,
                    ShapeRecord::RoundedRect {
                        local_rect: Some(_),
                        ..
                    }
                );
                is_caret && tree.paint_anims.sample(idx, now).alpha > 0.0
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

/// Between quantum boundaries, the caret's anim must NOT contribute
/// damage — otherwise an unrelated 60 Hz wake source would force a
/// caret repaint on every frame, defeating the point of damage.
/// `DamageEngine` gates the anim-rect add on
/// `entry.anim.next_wake(prev_now) <= now`.
#[test]
fn caret_anim_does_not_damage_between_quantum_boundaries() {
    use crate::layout::types::display::Display;
    use crate::ui::frame_report::FrameReport;
    use std::time::Duration;

    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::new();
    let display = Display::from_physical(NARROW, 1.0);

    // Single recording site keeps `track_caller` happy — every
    // frame's `Panel::hstack` resolves to the same source location,
    // so the Panel's auto-id is stable and structural damage stays
    // empty unless something actually changed.
    fn record(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id_salt("anim-damage")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    }
    let frame = |ui: &mut Ui, buf: &mut String, t_secs: f32| -> FrameReport {
        let report = ui.frame(display, Duration::from_secs_f32(t_secs), &mut (), |ui| {
            record(ui, buf);
        });
        ui.frame_state.mark_submitted();
        report
    };

    // Frame 1: warm up so the editor's WidgetId is recorded.
    frame(&mut ui, &mut buf, 0.0);

    // Frame 2 (focus): click lands; caret anim registers with
    // started_at=0. First post-focus frame is structurally dirty
    // (chrome/state change) — we don't assert on it.
    click_at(&mut ui, Vec2::new(20.0, 20.0));
    frame(&mut ui, &mut buf, 0.0);

    // Frame 3 mid-half-period (t=0.2 of a 0.5s half-period). Caret
    // quantum unchanged since prev frame (t=0); `next_wake(0) = 0.5`
    // which isn't `<= 0.2` → anim contributes no damage. No other
    // source of damage either → report damage is `None`.
    let report = frame(&mut ui, &mut buf, 0.2);
    assert!(
        report.plan.is_none(),
        "mid-phase frame should not damage the caret rect (got {:?})",
        report.plan,
    );

    // Frame 4 across the half-period boundary (t=0.6). prev_now=0.2;
    // `next_wake(0.2) = 0.5` which IS `<= 0.6` → quantum flipped
    // → caret rect joins damage.
    let report = frame(&mut ui, &mut buf, 0.6);
    assert!(
        report.plan.is_some(),
        "crossing a phase boundary must damage the caret rect",
    );
}

/// The paint-anim-only short-circuit must fire on a cross-boundary
/// wake when nothing else changed. Asserts the dedicated path
/// (skipped pre_record + closure + post_record + layout + cascades
/// + finalize) ran AND the damage region still contained the caret.
#[test]
fn caret_blink_fires_paint_anim_only_short_circuit() {
    use crate::layout::types::display::Display;
    use crate::ui::frame_report::{FrameProcessing, FrameReport};
    use std::time::Duration;

    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::new();
    let display = Display::from_physical(NARROW, 1.0);

    fn record(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id_salt("short-circuit")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    }
    let frame = |ui: &mut Ui, buf: &mut String, t_secs: f32| -> FrameReport {
        let r = ui.frame(display, Duration::from_secs_f32(t_secs), &mut (), |ui| {
            record(ui, buf);
        });
        ui.frame_state.mark_submitted();
        r
    };

    // Warmup + focus on the editor.
    frame(&mut ui, &mut buf, 0.0);
    click_at(&mut ui, Vec2::new(20.0, 20.0));
    frame(&mut ui, &mut buf, 0.0);

    // Mid-phase frame — wake hasn't fired, full record path runs.
    let report = frame(&mut ui, &mut buf, 0.2);
    assert_ne!(
        report.processing,
        FrameProcessing::PaintOnly,
        "mid-phase frame must not take the short-circuit path",
    );

    // Cross-boundary wake fires. Nothing else changed → short-circuit.
    let report = frame(&mut ui, &mut buf, 0.6);
    assert_eq!(
        report.processing,
        FrameProcessing::PaintOnly,
        "cross-boundary wake must take the short-circuit path",
    );
    assert!(
        report.plan.is_some(),
        "short-circuit must still surface anim damage",
    );

    // Input arrival (pointer move) on a follow-up frame disqualifies
    // the next short-circuit even if a wake is firing — the closure
    // must run so widgets can react.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 20.0)));
    let report = frame(&mut ui, &mut buf, 1.1);
    assert_ne!(
        report.processing,
        FrameProcessing::PaintOnly,
        "input arrival must force the full record path",
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
