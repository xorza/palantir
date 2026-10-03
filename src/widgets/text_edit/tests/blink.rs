use crate::scene::tree::node_id::NodeId;
use crate::ui::frame_report::FramePaint;
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
    use crate::scene::tree::iter::TreeItem;
    use crate::shape::paint::quad_shape::QuadShape;
    use crate::shape::record::ShapeRecord;
    use crate::shape::rect::RectKind;

    let tree = ui.tree(Layer::Main);
    let now = ui.now();
    let mut paint_anims = tree.paint_anims.cursor();
    // The caret is recorded on the block child that carries the field's
    // alignment, not on the field's own node — see [`block_of`].
    tree.tree_items(block_of(ui, leaf))
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

/// Caret blink: visible for the first half-period, hidden for the
/// second, repeats. Reset to "visible" by any caret / selection /
/// text change. Off entirely when the editor isn't focused.
#[test]
fn caret_blinks_on_and_off_while_focused() {
    fn body(ui: &mut Ui, buf: &mut String) -> NodeId {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                TextEdit::new(buf)
                    .id(WidgetId::from_hash("blink-ed"))
                    .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                    .show(ui)
                    .response
                    .node()
            })
            .inner
    }

    let mut h = UiHarness::new(NARROW);
    let mut buf = String::new();

    // Frame 1: record editor unfocused. The tree records the same shape
    // every frame, so the editor's node is the same on each; a
    // paint-only frame records none.
    let leaf = h
        .at(Duration::from_secs_f32(0.0))
        .frame_value(|ui| body(ui, &mut buf));
    assert!(
        !caret_painted(&h.ui, leaf),
        "unfocused editor paints no caret",
    );

    // Click focuses; caret jumps to byte 0 (empty buf). Drive a fresh
    // frame at t=0 so the input pass drains the click. caret_changed =
    // true → last_caret_change = 0; elapsed = 0; phase 0; visible.
    h.click_at(Vec2::new(20.0, 20.0));
    h.at(Duration::from_secs_f32(0.0)).frame(|ui| {
        body(ui, &mut buf);
    });
    assert!(caret_painted(&h.ui, leaf), "freshly focused: caret visible");

    // Still inside the first half-period.
    h.at(Duration::from_secs_f32(0.3)).frame(|ui| {
        body(ui, &mut buf);
    });
    assert!(
        caret_painted(&h.ui, leaf),
        "first half of blink cycle: caret visible",
    );

    // Crossed into the hidden half.
    h.at(Duration::from_secs_f32(0.7)).frame(|ui| {
        body(ui, &mut buf);
    });
    assert!(
        !caret_painted(&h.ui, leaf),
        "second half of blink cycle: caret hidden",
    );

    // One full period later: visible again.
    h.at(Duration::from_secs_f32(1.2)).frame(|ui| {
        body(ui, &mut buf);
    });
    assert!(
        caret_painted(&h.ui, leaf),
        "after a full period: caret visible again",
    );

    // Typing during a hidden phase must snap the caret back on.
    h.at(Duration::from_secs_f32(1.7)).frame(|ui| {
        body(ui, &mut buf);
    });
    assert!(
        !caret_painted(&h.ui, leaf),
        "precondition: hidden phase before keystroke",
    );
    h.key(Key::Char('a'));
    h.at(Duration::from_secs_f32(1.75)).frame(|ui| {
        body(ui, &mut buf);
    });
    assert!(
        caret_painted(&h.ui, leaf),
        "keystroke resets blink: caret immediately visible",
    );

    // Long idle: blink stops scheduling and caret stays visible so
    // an unattended focused editor doesn't keep the host repainting
    // at 2 Hz forever. 98.25s past the last change is far beyond
    // `BLINK_STOP_AFTER_IDLE`, and lands on an *odd* half-period —
    // parity says hidden, the settle overrides it.
    let report = h.at(Duration::from_secs_f32(100.0)).frame(|ui| {
        body(ui, &mut buf);
    });
    assert!(
        caret_painted(&h.ui, leaf),
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
    fn body(ui: &mut Ui, buf: &mut String) -> NodeId {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                TextEdit::new(buf)
                    .id(WidgetId::from_hash("caret-move-blink"))
                    .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                    .show(ui)
                    .response
                    .node()
            })
            .inner
    }

    let mut h = UiHarness::new(NARROW);
    // Long enough that a click near the left edge lands well short of
    // the end, so `End` is guaranteed to move the caret.
    let mut buf = String::from("abcdefghij");

    // The tree records the same shape every frame, so the editor's node
    // is the same on each; a paint-only frame records none.
    let leaf = h
        .at(Duration::from_secs_f32(0.0))
        .frame_value(|ui| body(ui, &mut buf));
    h.click_at(Vec2::new(20.0, 20.0));
    h.at(Duration::from_secs_f32(0.0)).frame(|ui| {
        body(ui, &mut buf);
    });
    let caret_at_click = h
        .state::<TextEditState>(WidgetId::from_hash("caret-move-blink"))
        .edit
        .caret;
    assert!(
        caret_at_click < buf.len(),
        "click must land short of the end for `End` to move the caret",
    );

    // 0.7s past the focus reset — one full half-period in, so the
    // blink is in its hidden phase.
    h.at(Duration::from_secs_f32(0.7)).frame(|ui| {
        body(ui, &mut buf);
    });
    assert!(
        !caret_painted(&h.ui, leaf),
        "precondition: hidden phase before the caret moves",
    );

    h.key(Key::End);
    h.at(Duration::from_secs_f32(0.75)).frame(|ui| {
        body(ui, &mut buf);
    });
    let state = h
        .state::<TextEditState>(WidgetId::from_hash("caret-move-blink"))
        .clone();
    assert_eq!(buf, "abcdefghij", "`End` must not edit the buffer");
    assert_eq!(state.edit.caret, buf.len(), "`End` moves caret to the end");
    assert!(
        caret_painted(&h.ui, leaf),
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
    let mut h = UiHarness::new(NARROW);
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
    h.at(Duration::from_secs_f32(0.0))
        .frame(|ui| record(ui, &mut buf));

    // Frame 2 (focus): click lands; caret anim registers with
    // started_at=0. First post-focus frame is structurally dirty
    // (chrome/state change) — we don't assert on it.
    h.click_at(Vec2::new(20.0, 20.0));
    h.at(Duration::from_secs_f32(0.0))
        .frame(|ui| record(ui, &mut buf));

    // Frame 3 mid-half-period (t=0.2 of a 0.5s half-period). Caret
    // quantum unchanged since prev frame (t=0); `next_wake(0) = 0.5`
    // which isn't `<= 0.2` → anim contributes no damage. No other
    // source of damage either → report damage is `None`.
    let report = h
        .at(Duration::from_secs_f32(0.2))
        .frame(|ui| record(ui, &mut buf));
    assert_eq!(
        report.paint(),
        FramePaint::Skip,
        "mid-phase frame should not damage the caret rect",
    );

    // Frame 4 across the half-period boundary (t=0.6). prev_now=0.2;
    // `next_wake(0.2) = 0.5` which IS `<= 0.6` → quantum flipped
    // → caret rect joins damage.
    let report = h
        .at(Duration::from_secs_f32(0.6))
        .frame(|ui| record(ui, &mut buf));
    assert_eq!(
        report.paint(),
        FramePaint::Partial,
        "crossing a phase boundary damages the caret rect, and only it",
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
    let mut h = UiHarness::new(NARROW);
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
    h.at(Duration::from_secs_f32(100.0))
        .frame(|ui| body(ui, &mut buf));

    // Click to focus on the empty buffer at t=100s. Caret lands at
    // byte 0 (unchanged from default), selection unchanged, text
    // unchanged — only the focus edge fires.
    h.click_at(Vec2::new(20.0, 20.0));
    let r = h
        .at(Duration::from_secs_f32(100.0))
        .frame(|ui| body(ui, &mut buf));

    // Focus rising edge must reset blink: anim registered → wake
    // scheduled at the next half-period boundary.
    // Restarted at the focus, so the next flip is half a 0.5 s period in.
    assert_eq!(
        r.repaint_after,
        Some(Duration::from_secs_f32(100.5)),
        "focus gain must restart blink scheduling regardless of caret movement",
    );
}

/// Focused TextEdit must keep the host's repaint loop alive — without
/// the wake schedule, the blink would freeze on whichever phase the
/// last frame landed on.
#[test]
fn focused_text_edit_schedules_blink_wake() {
    let mut h = UiHarness::new(NARROW);
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
    assert_eq!(
        report.repaint_after,
        Some(Duration::from_millis(500)),
        "focused editor schedules a blink wake at the first half-period flip",
    );
}
