use crate::ui::frame_report::FramePaint;
use crate::widgets::text_edit::tests::*;
use std::time::Duration;

/// "Painted" means the caret rect is present and its `PaintAnimation` (if any)
/// samples visible now. On a focused, unselected editor the caret is the only
/// rounded rect with `local_rect: Some(...)`.
fn caret_painted(ui: &Ui, leaf: NodeId) -> bool {
    use crate::scene::tree::iter::TreeItem;
    use crate::shape::paint::quad_shape::QuadShape;
    use crate::shape::rect::RectKind;

    let tree = ui.tree(Layer::Main);
    let now = ui.now();
    let mut paint_anims = tree.paint_anims.cursor();
    // The caret is on the block child carrying the field's alignment; see [`block_of`].
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

/// Caret blink: visible the first half-period, hidden the second, repeating.
/// Reset to visible by any caret / selection / text change; off when unfocused.
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

    // Frame 1: record the editor unfocused. The editor's node is the same on
    // each frame; a paint-only frame records none.
    let leaf = h
        .at(Duration::from_secs_f32(0.0))
        .frame_value(|ui| body(ui, &mut buf));
    assert!(
        !caret_painted(&h.ui, leaf),
        "unfocused editor paints no caret",
    );

    // Click focuses; caret at byte 0. A fresh frame at t=0 drains the click:
    // last_caret_change = 0, phase 0, visible.
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

    // Long idle: blink stops scheduling and the caret stays visible so an
    // unattended editor doesn't repaint at 2 Hz forever. t=100s is far past
    // `BLINK_STOP_AFTER_IDLE` on an odd half-period, so parity says hidden and the
    // settle overrides it.
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

/// Caret motion alone resets the blink: `End` moves the caret, leaving text
/// alone, so the reset rides on `caret_moved` with `edited` and `focus_gained`
/// false. Separate from the sweep above because it moves that test's
/// last-change timestamp.
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
    // Long enough that a click near the left edge is short of the end.
    let mut buf = String::from("abcdefghij");

    // The editor's node is the same on each frame; a paint-only frame records none.
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

    // 0.7s past the focus reset: the hidden phase.
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

/// Between quantum boundaries the caret anim must not add damage, or an
/// unrelated 60 Hz wake would repaint the caret every frame. `DamageEngine`
/// gates it on `entry.anim.next_wake(prev_now) <= now`.
#[test]
fn caret_anim_does_not_damage_between_quantum_boundaries() {
    // A single recording site keeps the Panel's auto-id stable, so structural
    // damage stays empty unless something changed.
    fn record(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("anim-damage"))
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    }

    let mut h = UiHarness::new(NARROW);
    let mut buf = String::new();

    h.at(Duration::from_secs_f32(0.0))
        .frame(|ui| record(ui, &mut buf));

    // Frame 2 (focus): the caret anim registers with started_at=0. This frame is
    // structurally dirty; not asserted.
    h.click_at(Vec2::new(20.0, 20.0));
    h.at(Duration::from_secs_f32(0.0))
        .frame(|ui| record(ui, &mut buf));

    // Frame 3 at t=0.2: `next_wake(0) = 0.5` is not `<= 0.2`, so no anim damage
    // and no other source: report damage is `None`.
    let report = h
        .at(Duration::from_secs_f32(0.2))
        .frame(|ui| record(ui, &mut buf));
    assert_eq!(
        report.paint(),
        FramePaint::Skip,
        "mid-phase frame should not damage the caret rect",
    );

    // Frame 4 at t=0.6: `next_wake(0.2) = 0.5` is `<= 0.6`, so the caret rect
    // joins damage.
    let report = h
        .at(Duration::from_secs_f32(0.6))
        .frame(|ui| record(ui, &mut buf));
    assert_eq!(
        report.paint(),
        FramePaint::Partial,
        "crossing a phase boundary damages the caret rect, and only it",
    );
}

/// Focusing at any wall-clock time must restart the blink even if caret,
/// selection and text are unchanged; otherwise a focus past
/// `BLINK_STOP_AFTER_IDLE` registers an anim already past its stop and the
/// caret stays solid.
#[test]
fn focus_gain_resets_blink_even_without_caret_change() {
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("refocus-blink"))
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    }

    let mut h = UiHarness::new(NARROW);
    let mut buf = String::new();

    // Warm up unfocused, past `BLINK_STOP_AFTER_IDLE`, so a stale
    // `last_caret_change=0` would put the blink past its cliff.
    h.at(Duration::from_secs_f32(100.0))
        .frame(|ui| body(ui, &mut buf));

    // Click to focus at t=100s; only the focus edge fires.
    h.click_at(Vec2::new(20.0, 20.0));
    let r = h
        .at(Duration::from_secs_f32(100.0))
        .frame(|ui| body(ui, &mut buf));

    // The focus edge resets the blink: the next flip is half a 0.5 s period in.
    assert_eq!(
        r.repaint_after,
        Some(Duration::from_secs_f32(100.5)),
        "focus gain must restart blink scheduling regardless of caret movement",
    );
}

/// A focused TextEdit keeps the host's repaint loop alive via the wake
/// schedule; without it the blink freezes on the last phase.
#[test]
fn focused_text_edit_schedules_blink_wake() {
    let mut h = UiHarness::new(NARROW);
    let mut buf = String::new();

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

    // Focus, then another frame: a wake is requested at the next phase boundary.
    h.click_at(Vec2::new(20.0, 20.0));
    let report = h.frame(&mut scene);
    assert_eq!(
        report.repaint_after,
        Some(Duration::from_millis(500)),
        "focused editor schedules a blink wake at the first half-period flip",
    );
}
