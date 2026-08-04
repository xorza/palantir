use crate::scene::tree::record::NodeId;
use crate::ui::frame_report::FrameReport;
use crate::widgets::text_edit::tests::*;
use std::time::Duration;

/// Caret is the only rounded rect with `local_rect: Some(...)` on a
/// focused, unselected editor — `Background` routes through `chrome`
/// (no shape), selection wash is absent without a selection.
/// Post-`PaintAnim`-migration the rect is always present when focused;
/// the encoder hides it via the attached `PaintAnim`. "Painted" means
/// "rect present AND its anim (if any) samples to visible at the
/// current time".
fn caret_painted(ui: &Ui, leaf: NodeId) -> bool {
    use crate::scene::shapes::paint::QuadShape;
    use crate::scene::shapes::record::ShapeRecord;
    use crate::scene::tree::iter::{TreeItem, TreeItems};
    use crate::shape::rect::RectKind;

    let tree = &ui.forest.trees[Layer::Main];
    let now = ui.frame_runtime.time;
    let mut paint_anims = tree.paint_anims.cursor();
    TreeItems::new(&tree.records, &tree.shapes.records, leaf)
        .filter_map(|item| match item {
            TreeItem::ShapeRecord(idx, s) => Some((idx, s)),
            TreeItem::Child(_) => None,
        })
        .any(|(idx, s)| {
            let is_caret = matches!(
                s,
                ShapeRecord::Quad(QuadShape::Rect {
                    kind: RectKind::Rounded,
                    local_rect: Some(_),
                    ..
                })
            );
            is_caret && paint_anims.sample(idx, now).alpha > 0.0
        })
}

fn record_at_secs(h: &mut UiHarness, now_secs: f32, mut f: impl FnMut(&mut Ui)) -> FrameReport {
    h.at(Duration::from_secs_f32(now_secs)).frame(|ui| f(ui))
}

/// Caret blink: visible for the first half-period, hidden for the
/// second, repeats. Reset to "visible" by any caret / selection /
/// text change. Off entirely when the editor isn't focused.
#[test]
fn caret_blinks_on_and_off_while_focused() {
    fn body(ui: &mut Ui, buf: &mut String, leaf: &mut Option<NodeId>) {
        Panel::hstack().auto_id().show(ui, |ui| {
            *leaf = Some(
                TextEdit::new(buf)
                    .id(WidgetId::from_hash("blink-ed"))
                    .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                    .show(ui)
                    .response
                    .node(),
            );
        });
    }

    let mut h = ui_at_no_cosmic(NARROW);
    let mut buf = String::new();
    let mut leaf = None;

    // Frame 1: record editor unfocused.
    record_at_secs(&mut h, 0.0, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        !caret_painted(&h.ui, leaf.unwrap()),
        "unfocused editor paints no caret",
    );

    // Click focuses; caret jumps to byte 0 (empty buf). Drive a fresh
    // frame at t=0 so run_input drains the click. caret_changed =
    // true → last_caret_change = 0; elapsed = 0; phase 0; visible.
    h.click_at(Vec2::new(20.0, 20.0));
    record_at_secs(&mut h, 0.0, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        caret_painted(&h.ui, leaf.unwrap()),
        "freshly focused: caret visible",
    );

    // Still inside the first half-period.
    record_at_secs(&mut h, 0.3, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        caret_painted(&h.ui, leaf.unwrap()),
        "first half of blink cycle: caret visible",
    );

    // Crossed into the hidden half.
    record_at_secs(&mut h, 0.7, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        !caret_painted(&h.ui, leaf.unwrap()),
        "second half of blink cycle: caret hidden",
    );

    // One full period later: visible again.
    record_at_secs(&mut h, 1.2, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        caret_painted(&h.ui, leaf.unwrap()),
        "after a full period: caret visible again",
    );

    // Typing during a hidden phase must snap the caret back on.
    record_at_secs(&mut h, 1.7, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        !caret_painted(&h.ui, leaf.unwrap()),
        "precondition: hidden phase before keystroke",
    );
    h.key(Key::Char('a'));
    record_at_secs(&mut h, 1.75, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        caret_painted(&h.ui, leaf.unwrap()),
        "keystroke resets blink: caret immediately visible",
    );

    // Long idle: blink stops scheduling and caret stays visible so
    // an unattended focused editor doesn't keep the host repainting
    // at 2 Hz forever. 98.25s past the last change is far beyond
    // `BLINK_STOP_AFTER_IDLE`, and lands on an *odd* half-period —
    // parity says hidden, the settle overrides it.
    let report = record_at_secs(&mut h, 100.0, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        caret_painted(&h.ui, leaf.unwrap()),
        "long-idle blink stops on the visible phase",
    );
    assert_eq!(
        report.repaint_after, None,
        "a settled caret must stop asking the host for frames",
    );
}

/// Caret *motion* with no edit resets the blink, on its own: `End`
/// walks the caret to the buffer end and leaves the text alone, so the
/// reset rides on `caret_moved` with `edited` and `gained_focus` both
/// false. Separate from the sweep above because the reset it performs
/// moves that test's last-change timestamp, and its long-idle tail
/// assertion is phase-sensitive to exactly that.
#[test]
fn caret_motion_alone_resets_blink() {
    fn body(ui: &mut Ui, buf: &mut String, leaf: &mut Option<NodeId>) {
        Panel::hstack().auto_id().show(ui, |ui| {
            *leaf = Some(
                TextEdit::new(buf)
                    .id(WidgetId::from_hash("caret-move-blink"))
                    .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                    .show(ui)
                    .response
                    .node(),
            );
        });
    }

    let mut h = ui_at_no_cosmic(NARROW);
    // Long enough that a click near the left edge lands well short of
    // the end, so `End` is guaranteed to move the caret.
    let mut buf = String::from("abcdefghij");
    let mut leaf = None;

    record_at_secs(&mut h, 0.0, |ui| body(ui, &mut buf, &mut leaf));
    h.click_at(Vec2::new(20.0, 20.0));
    record_at_secs(&mut h, 0.0, |ui| body(ui, &mut buf, &mut leaf));
    let caret_at_click =
        h.ui.state_mut::<TextEditState>(WidgetId::from_hash("caret-move-blink"))
            .edit
            .caret;
    assert!(
        caret_at_click < buf.len(),
        "click must land short of the end for `End` to move the caret",
    );

    // 0.7s past the focus reset — one full half-period in, so the
    // blink is in its hidden phase.
    record_at_secs(&mut h, 0.7, |ui| body(ui, &mut buf, &mut leaf));
    assert!(
        !caret_painted(&h.ui, leaf.unwrap()),
        "precondition: hidden phase before the caret moves",
    );

    h.key(Key::End);
    record_at_secs(&mut h, 0.75, |ui| body(ui, &mut buf, &mut leaf));
    let state =
        h.ui.state_mut::<TextEditState>(WidgetId::from_hash("caret-move-blink"))
            .clone();
    assert_eq!(buf, "abcdefghij", "`End` must not edit the buffer");
    assert_eq!(state.edit.caret, buf.len(), "`End` moves caret to the end");
    assert!(
        caret_painted(&h.ui, leaf.unwrap()),
        "caret movement alone resets blink: caret immediately visible",
    );
}

/// Between quantum boundaries, the caret's anim must NOT contribute
/// damage — otherwise an unrelated 60 Hz wake source would force a
/// caret repaint on every frame, defeating the point of damage.
/// `DamageEngine` gates the anim-rect add on
/// `entry.anim.next_wake(prev_now) <= now`.
#[test]
fn caret_anim_does_not_damage_between_quantum_boundaries() {
    let mut h = ui_at_no_cosmic(NARROW);
    let mut buf = String::new();

    // Single recording site keeps `track_caller` happy — every
    // frame's `Panel::hstack` resolves to the same source location,
    // so the Panel's auto-id is stable and structural damage stays
    // empty unless something actually changed.
    fn record(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("anim-damage"))
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    }

    // Frame 1: warm up so the editor's WidgetId is recorded.
    record_at_secs(&mut h, 0.0, |ui| record(ui, &mut buf));

    // Frame 2 (focus): click lands; caret anim registers with
    // started_at=0. First post-focus frame is structurally dirty
    // (chrome/state change) — we don't assert on it.
    h.click_at(Vec2::new(20.0, 20.0));
    record_at_secs(&mut h, 0.0, |ui| record(ui, &mut buf));

    // Frame 3 mid-half-period (t=0.2 of a 0.5s half-period). Caret
    // quantum unchanged since prev frame (t=0); `next_wake(0) = 0.5`
    // which isn't `<= 0.2` → anim contributes no damage. No other
    // source of damage either → report damage is `None`.
    let report = record_at_secs(&mut h, 0.2, |ui| record(ui, &mut buf));
    assert!(
        report.plan.is_none(),
        "mid-phase frame should not damage the caret rect (got {:?})",
        report.plan,
    );

    // Frame 4 across the half-period boundary (t=0.6). prev_now=0.2;
    // `next_wake(0.2) = 0.5` which IS `<= 0.6` → quantum flipped
    // → caret rect joins damage.
    let report = record_at_secs(&mut h, 0.6, |ui| record(ui, &mut buf));
    assert!(
        report.plan.is_some(),
        "crossing a phase boundary must damage the caret rect",
    );
}

/// Focusing a TextEdit at any wall-clock time must restart the blink,
/// even when the caret/selection/text didn't change. Otherwise a fresh
/// focus past `BLINK_STOP_AFTER_IDLE` registers an anim that is
/// already past its own stop, so it settles solid immediately — caret
/// stays solid until the user types or moves the caret. Regression for
/// the "caret doesn't blink unless I move the mouse" bug.
#[test]
fn focus_gain_resets_blink_even_without_caret_change() {
    let mut h = ui_at_no_cosmic(NARROW);
    let mut buf = String::new();

    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("refocus-blink"))
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    }
    // Warm up — unfocused, well past `BLINK_STOP_AFTER_IDLE` so any
    // stale `last_caret_change=0` would put the blink past its cliff.
    record_at_secs(&mut h, 100.0, |ui| body(ui, &mut buf));

    // Click to focus on the empty buffer at t=100s. Caret lands at
    // byte 0 (unchanged from default), selection unchanged, text
    // unchanged — only the focus edge fires.
    h.click_at(Vec2::new(20.0, 20.0));
    let r = record_at_secs(&mut h, 100.0, |ui| body(ui, &mut buf));

    // Focus rising edge must reset blink: anim registered → wake
    // scheduled at the next half-period boundary.
    assert!(
        r.repaint_after.is_some(),
        "focus gain must restart blink scheduling regardless of caret movement",
    );
}

/// Focused TextEdit must keep the host's repaint loop alive — without
/// the wake schedule, the blink would freeze on whichever phase the
/// last frame landed on.
#[test]
fn focused_text_edit_schedules_blink_wake() {
    let mut h = ui_at_no_cosmic(NARROW);
    let mut buf = String::new();

    // Unfocused: no blink schedule.
    let mut scene = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(&mut buf)
                .id(WidgetId::from_hash("blink-wake"))
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    };
    let report = h.frame(&mut scene);
    assert_eq!(
        report.repaint_after, None,
        "unfocused editor doesn't schedule blink wakes",
    );

    // Focus, then drive another frame — now the scheduler should
    // request a wake at the next phase boundary.
    h.click_at(Vec2::new(20.0, 20.0));
    let report = h.frame(&mut scene);
    assert!(
        report.repaint_after.is_some(),
        "focused editor schedules a blink wake",
    );
}
